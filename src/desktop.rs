//! Plays tape steps on a desktop: timing, glides and waits, through a [`Platform`].

use std::num::NonZeroU32;
use std::path::Path;
use std::time::Duration;

use crate::motion;
pub use crate::platform::Error;
use crate::platform::{pick_control, Platform};
use crate::tape::{
    AppName, ControlLabel, DockItem, KeyChord, Point, ShellCommand, Step, Target, WindowName,
};

/// Pointer updates per second while gliding.
const MOVE_RATE_HZ: u32 = 60;
/// How long the pointer rests on a target before pressing, so the app has seen it arrive.
const HOVER: Duration = Duration::from_millis(150);
/// How often waits on the screen poll.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// The longest a step runs between checks that the recording is alive.
const ALIVE_INTERVAL: Duration = Duration::from_secs(1);
/// How often `WaitUntil` reruns its command.
pub(crate) const UNTIL_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Drives a desktop, tracking speed settings between steps.
pub struct Desktop<P: Platform> {
    platform: P,
    typing_delay: Duration,
    move_duration: Duration,
}

impl<P: Platform> Desktop<P> {
    /// A desktop typing at 50 ms per key and gliding in 600 ms.
    pub fn new(platform: P) -> Self {
        Self {
            platform,
            typing_delay: Duration::from_millis(50),
            move_duration: Duration::from_millis(600),
        }
    }

    /// Consumes the desktop, returning its platform.
    pub fn into_platform(self) -> P {
        self.platform
    }

    /// Plays one step, ending it early once `alive` reports false.
    ///
    /// # Errors
    ///
    /// As [`Desktop::play`], plus [`Error::Interrupted`] once `alive` reports false.
    pub fn play_while(
        &mut self,
        step: &Step,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        match step {
            Step::TypingSpeed(delay) => self.typing_delay = *delay,
            Step::MoveDuration(duration) => self.move_duration = *duration,
            Step::Sleep(duration) => self.sleep_while(*duration, alive)?,
            Step::MoveTo(target) => self.glide(target, alive)?,
            Step::Click(target) => {
                self.glide(target, alive)?;
                self.platform.sleep(HOVER);
                self.platform.click()?;
            }
            Step::WaitWindow { name, timeout } => self.wait_window(name, *timeout, alive)?,
            Step::WaitUntil {
                command,
                fail_when,
                timeout,
            } => self.wait_until(command, fail_when.as_ref(), *timeout, alive)?,
            Step::Focus { app, timeout } => self.focus(app, *timeout, alive)?,
            // Warps and bars mark the recording; the caller times them.
            Step::Warp { .. } | Step::EndWarp | Step::Bar(_) => {}
            Step::Type(text) => self.type_text(text)?,
            Step::Paste(text) => self.platform.paste(text)?,
            Step::Key(chord) => self.press(chord)?,
        }
        Ok(())
    }

    /// Plays one step.
    ///
    /// # Errors
    ///
    /// Fails if the platform fails or a wait times out.
    pub fn play(&mut self, step: &Step) -> Result<(), Error> {
        self.play_while(step, &mut || true)
    }

    /// Saves the screen to `path` as PNG, silently.
    ///
    /// # Errors
    ///
    /// Fails if the capture fails.
    pub fn screenshot(&mut self, path: &str) -> Result<(), Error> {
        self.platform.screenshot(Path::new(path))
    }

    /// Sleeps for `duration` in slices of at most [`ALIVE_INTERVAL`], checking `alive` before each.
    fn sleep_while(
        &mut self,
        duration: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let start = self.platform.elapsed();
        loop {
            if !alive() {
                return Err(Error::Interrupted);
            }
            let left = duration.saturating_sub(self.platform.elapsed().saturating_sub(start));
            if left.is_zero() {
                return Ok(());
            }
            self.platform.sleep(left.min(ALIVE_INTERVAL));
        }
    }

    /// Sleeps until `offset` after `start` on the platform's clock.
    fn sleep_until(&mut self, start: Duration, offset: Duration) {
        let passed = self.platform.elapsed().saturating_sub(start);
        let remaining = offset.saturating_sub(passed);
        if !remaining.is_zero() {
            self.platform.sleep(remaining);
        }
    }

    /// Types `text` one character at a time, paced by the typing delay.
    fn type_text(&mut self, text: &str) -> Result<(), Error> {
        let start = self.platform.elapsed();
        for (i, c) in (0u32..).zip(text.chars()) {
            // Keystroke `i` starts `i` typing delays in.
            self.sleep_until(start, self.typing_delay.saturating_mul(i));
            self.platform.type_char(c)?;
        }
        Ok(())
    }

    fn press(&mut self, chord: &KeyChord) -> Result<(), Error> {
        self.platform.key(chord)
    }

    /// Moves the pointer to `target` along an eased path at [`MOVE_RATE_HZ`].
    fn glide(&mut self, target: &Target, alive: &mut dyn FnMut() -> bool) -> Result<(), Error> {
        let to = match target {
            Target::Point(point) => *point,
            Target::Dock(item) => self.dock_point(item)?,
            Target::Control { label, timeout } => self.control_point(label, *timeout, alive)?,
        };
        let from = self.platform.pointer()?;
        let ticks = self.move_duration.as_millis() * u128::from(MOVE_RATE_HZ) / 1000;
        let steps =
            NonZeroU32::new(u32::try_from(ticks).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN);
        let points = motion::path(from, to, steps);
        let start = self.platform.elapsed();
        let mut i = 1;
        loop {
            // Each move is due at a fixed offset from the glide's start.
            self.sleep_until(start, self.move_duration * i / steps.get());
            let point = usize::try_from(i - 1).ok().and_then(|n| points.get(n));
            self.platform.move_to(point.copied().unwrap_or(to))?;
            if i == steps.get() {
                return Ok(());
            }
            // Points already due are skipped, so slow moves keep the glide's duration.
            let elapsed = self.platform.elapsed().saturating_sub(start).as_nanos();
            let due = elapsed * u128::from(steps.get()) / self.move_duration.as_nanos().max(1);
            i = u32::try_from(due)
                .unwrap_or(u32::MAX)
                .clamp(i + 1, steps.get());
        }
    }

    /// The centre of `item`'s icon, from the Dock's own geometry.
    fn dock_point(&mut self, item: &DockItem) -> Result<Point, Error> {
        self.platform
            .dock()?
            .into_iter()
            .find_map(|(name, centre)| (name == item.name()).then_some(centre))
            .ok_or_else(|| Error::NotInDock(item.name().to_owned()))
    }

    /// Polls until a window shows a button labelled `label`, then returns its centre.
    ///
    /// A label shown by several apps resolves to the frontmost one's.
    fn control_point(
        &mut self,
        label: &ControlLabel,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<Point, Error> {
        self.poll(POLL_INTERVAL, timeout, alive, |platform, limit| {
            pick_control(&platform.buttons(limit)?, label.as_str())
        })?
        .ok_or_else(|| Error::ControlNotFound(label.as_str().to_owned()))
    }

    /// Activates `app` and polls until the platform reports it frontmost.
    fn focus(
        &mut self,
        app: &AppName,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        self.poll(POLL_INTERVAL, timeout, alive, |platform, limit| {
            Ok(platform.focus(app, limit)?.then_some(()))
        })?
        .ok_or_else(|| Error::NotFrontmost(app.as_str().to_owned()))
    }

    /// Polls until a visible window's title contains `name`, ignoring case.
    fn wait_window(
        &mut self,
        name: &WindowName,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let needle = name.as_str().to_lowercase();
        self.poll(POLL_INTERVAL, timeout, alive, |platform, limit| {
            let titles = platform.windows(limit)?;
            Ok(titles
                .iter()
                .any(|title| title.to_lowercase().contains(&needle))
                .then_some(()))
        })?
        .ok_or_else(|| Error::WindowNotFound(name.as_str().to_owned()))
    }

    /// Polls the shell command until it succeeds, or until `fail_when` succeeds first.
    fn wait_until(
        &mut self,
        command: &ShellCommand,
        fail_when: Option<&ShellCommand>,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        self.poll(UNTIL_POLL_INTERVAL, timeout, alive, |platform, limit| {
            let started = platform.elapsed();
            if platform.shell(command, limit)? {
                return Ok(Some(()));
            }
            let Some(fail_when) = fail_when else {
                return Ok(None);
            };
            // The failure check shares the attempt's time limit.
            let spent = platform.elapsed().saturating_sub(started);
            let left = limit.map(|limit| limit.saturating_sub(spent));
            if platform.shell(fail_when, left)? {
                return Err(Error::WaitFailed {
                    command: command.as_str().to_owned(),
                    fail_when: fail_when.as_str().to_owned(),
                });
            }
            Ok(None)
        })?
        .ok_or_else(|| Error::ConditionNotMet(command.as_str().to_owned()))
    }

    /// Reruns `attempt` every `interval` until it finds something or `timeout` passes.
    ///
    /// Each attempt gets the time left as its limit; one cut off at the deadline ends the wait.
    fn poll<T>(
        &mut self,
        interval: Duration,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
        mut attempt: impl FnMut(&mut P, Option<Duration>) -> Result<Option<T>, Error>,
    ) -> Result<Option<T>, Error> {
        let deadline = self.platform.elapsed().saturating_add(timeout);
        loop {
            if !alive() {
                return Err(Error::Interrupted);
            }
            let left = deadline.saturating_sub(self.platform.elapsed());
            match attempt(&mut self.platform, Some(left)) {
                Err(Error::TimedOut { .. }) => return Ok(None),
                Err(error) => return Err(error),
                Ok(Some(found)) => return Ok(Some(found)),
                Ok(None) => {}
            }
            let now = self.platform.elapsed();
            if now >= deadline {
                return Ok(None);
            }
            self.platform
                .sleep(interval.min(deadline.saturating_sub(now)));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::platform::fake::{Call, FakePlatform};
    use crate::platform::Control;
    use crate::tape;

    fn play(platform: FakePlatform, items: &str) -> Result<FakePlatform, Error> {
        let steps = tape::parse(&format!("output: t.mp4\nsteps:\n{items}"))
            .unwrap()
            .steps;
        let mut desktop = Desktop::new(platform);
        for step in &steps {
            desktop.play(step)?;
        }
        Ok(desktop.into_platform())
    }

    fn button(app: &str, frontmost: bool, label: &str, x: u32, y: u32) -> Control {
        Control {
            app: app.into(),
            frontmost,
            label: label.into(),
            centre: Point { x, y },
        }
    }

    fn moves(platform: &FakePlatform) -> Vec<Point> {
        platform
            .calls
            .iter()
            .filter_map(|call| match call {
                Call::MoveTo(point) => Some(*point),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn clicks_glide_at_sixty_hz_then_rest_and_click() {
        let platform = play(
            FakePlatform::default(),
            "  - set: { moveDuration: 500ms }\n  - click: { point: { x: 900, y: 600 } }\n",
        )
        .unwrap();
        let moves = moves(&platform);
        assert_eq!(moves.len(), 30);
        assert_eq!(moves.last(), Some(&Point { x: 900, y: 600 }));
        assert_eq!(platform.calls.last(), Some(&Call::Click));
        assert_eq!(platform.now(), Duration::from_millis(500) + HOVER);
    }

    #[test]
    fn buttons_are_awaited_then_the_frontmost_one_is_clicked() {
        let platform = FakePlatform {
            buttons: VecDeque::from([
                vec![],
                vec![
                    button("Docker", false, "Open", 10, 10),
                    button("Code", true, "Open", 500, 400),
                ],
            ]),
            ..FakePlatform::default()
        };
        let platform = play(platform, "  - click: { button: Open, timeout: 5s }\n").unwrap();
        assert_eq!(
            platform
                .calls
                .iter()
                .filter(|c| **c == Call::Buttons)
                .count(),
            2
        );
        assert_eq!(moves(&platform).last(), Some(&Point { x: 500, y: 400 }));
    }

    #[test]
    fn a_button_that_never_appears_times_out() {
        let err = play(
            FakePlatform::default(),
            "  - click: { button: Allow, timeout: 1s }\n",
        )
        .err();
        assert_eq!(err, Some(Error::ControlNotFound("Allow".into())));
    }

    #[test]
    fn dock_targets_come_from_the_platform() {
        let platform = FakePlatform {
            dock: vec![("Terminal".into(), Point { x: 1522, y: 1028 })],
            ..FakePlatform::default()
        };
        let platform = play(platform, "  - moveTo: { dock: Terminal }\n").unwrap();
        assert_eq!(moves(&platform).last(), Some(&Point { x: 1522, y: 1028 }));
        let err = play(FakePlatform::default(), "  - click: { dock: Safari }\n").err();
        assert_eq!(err, Some(Error::NotInDock("Safari".into())));
    }

    #[test]
    fn focus_polls_until_the_app_is_in_front() {
        let platform = FakePlatform {
            focus: VecDeque::from([false, true]),
            ..FakePlatform::default()
        };
        let platform = play(platform, "  - focus: Terminal\n").unwrap();
        assert_eq!(
            platform.calls,
            [
                Call::Focus("Terminal".into()),
                Call::Focus("Terminal".into())
            ]
        );
        assert_eq!(platform.now(), POLL_INTERVAL);
    }

    #[test]
    fn window_waits_match_titles_ignoring_case() {
        let platform = FakePlatform {
            windows: VecDeque::from([vec!["Finder".into()], vec!["VISUAL Studio Code".into()]]),
            ..FakePlatform::default()
        };
        let platform = play(platform, "  - waitWindow: { title: visual studio }\n").unwrap();
        assert_eq!(platform.calls, [Call::Windows, Call::Windows]);
    }

    #[test]
    fn a_hanging_query_ends_its_wait_at_the_deadline() {
        let platform = FakePlatform {
            query_cost: Duration::from_secs(600),
            ..FakePlatform::default()
        };
        let err = play(
            platform,
            "  - waitUntil: { command: curl -s http://stuck, timeout: 1m }\n",
        )
        .err();
        assert_eq!(
            err,
            Some(Error::ConditionNotMet("curl -s http://stuck".into()))
        );
    }

    #[test]
    fn shell_waits_poll_until_success() {
        let platform = FakePlatform {
            shell: VecDeque::from([false, false, true]),
            ..FakePlatform::default()
        };
        let platform = play(platform, "  - waitUntil: { command: test -f x }\n").unwrap();
        assert_eq!(platform.calls.len(), 3);
        assert_eq!(platform.now(), UNTIL_POLL_INTERVAL * 2);
    }

    #[test]
    fn a_failure_check_ends_the_wait_at_once() {
        let platform = FakePlatform {
            // Ready, failed: no, no; then no, yes.
            shell: VecDeque::from([false, false, false, true]),
            ..FakePlatform::default()
        };
        let mut desktop = Desktop::new(platform);
        let tape = "output: t.mp4\nsteps:\n  - waitUntil: { command: test -f ok, failWhen: test -f broken, timeout: 60m }\n";
        let err = tape::parse(tape)
            .unwrap()
            .steps
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(
            err,
            Some(Error::WaitFailed {
                command: "test -f ok".into(),
                fail_when: "test -f broken".into(),
            })
        );
        let platform = desktop.into_platform();
        assert_eq!(
            platform.calls,
            [
                Call::Shell("test -f ok".into()),
                Call::Shell("test -f broken".into()),
                Call::Shell("test -f ok".into()),
                Call::Shell("test -f broken".into()),
            ]
        );
        assert_eq!(platform.now(), UNTIL_POLL_INTERVAL);
    }

    #[test]
    fn success_wins_over_a_failure_check() {
        let platform = FakePlatform {
            shell: VecDeque::from([true, true]),
            ..FakePlatform::default()
        };
        let platform = play(
            platform,
            "  - waitUntil: { command: test -f ok, failWhen: test -f broken }\n",
        )
        .unwrap();
        assert_eq!(platform.calls, [Call::Shell("test -f ok".into())]);
    }

    #[test]
    fn a_failure_check_that_never_passes_leaves_the_timeout() {
        let err = play(
            FakePlatform::default(),
            "  - waitUntil: { command: test -f ok, failWhen: test -f broken, timeout: 2s }\n",
        )
        .err();
        assert_eq!(err, Some(Error::ConditionNotMet("test -f ok".into())));
    }

    #[test]
    fn typing_paces_one_character_per_delay() {
        let platform = play(
            FakePlatform::default(),
            "  - set: { typingSpeed: 80ms }\n  - type: hi!\n",
        )
        .unwrap();
        assert_eq!(
            platform.calls,
            [Call::Type('h'), Call::Type('i'), Call::Type('!')]
        );
        assert_eq!(platform.now(), Duration::from_millis(160));
    }

    #[test]
    fn paste_and_keys_go_straight_to_the_platform() {
        let platform = play(
            FakePlatform::default(),
            "  - paste: git clone x\n  - key: primary+k\n",
        )
        .unwrap();
        assert_eq!(platform.calls.len(), 2);
        assert_eq!(platform.calls[0], Call::Paste("git clone x".into()));
        assert!(matches!(&platform.calls[1], Call::Key(chord) if chord.contains("Primary")));
    }

    #[test]
    fn waits_stop_when_the_recording_ends() {
        let mut desktop = Desktop::new(FakePlatform::default());
        let steps =
            tape::parse("output: t.mp4\nsteps:\n  - waitUntil: { command: x, timeout: 1m }\n")
                .unwrap()
                .steps;
        let err = desktop.play_while(&steps[0], &mut || false).err();
        assert_eq!(err, Some(Error::Interrupted));
    }

    #[test]
    fn warp_markers_touch_nothing() {
        let platform = play(
            FakePlatform::default(),
            "  - warp: { label: x, length: 1s, steps: [] }\n",
        )
        .unwrap();
        assert_eq!(platform.calls, []);
    }
}
