//! Plays tape steps on the macOS desktop through `cliclick` and `osascript`.

use std::fmt;
use std::io;
use std::num::NonZeroU32;
use std::os::unix::process::CommandExt;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::motion;
use crate::tape::{DockItem, Key, KeyChord, Point, ShellCommand, Step, Target, WindowName};
use crate::units;

/// Pointer updates per second while gliding.
const MOVE_RATE_HZ: u32 = 60;
/// cliclick's message for a character outside the keyboard layout.
const UNTYPABLE: &str = "Unable to get key code for";
/// A pause after a lone key press, which cliclick only delivers when a command follows it.
const KEY_SETTLE: &str = "w:50";
/// How often `WaitWindow` polls for its window.
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// The longest a step runs between checks that the recording is alive.
const ALIVE_INTERVAL: Duration = Duration::from_secs(1);
/// How often `WaitUntil` reruns its command.
const UNTIL_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Lists every named Dock item as `name<TAB>x<TAB>y<TAB>width<TAB>height`, one per line.
const DOCK_SCRIPT: &[&str] = &[
    "set out to \"\"",
    "tell application \"System Events\" to tell process \"Dock\"",
    "repeat with e in UI elements of list 1",
    "set n to name of e",
    "if n is not missing value then",
    "set {x, y} to position of e",
    "set {w, h} to size of e",
    "set out to out & n & tab & x & tab & y & tab & w & tab & h & linefeed",
    "end if",
    "end repeat",
    "end tell",
    "return out",
];

/// Lists the named windows of foreground processes, one title per line.
const WINDOWS_SCRIPT: &[&str] = &[
    "set out to \"\"",
    "tell application \"System Events\"",
    "repeat with p in (every process whose background only is false)",
    "repeat with w in windows of p",
    "set t to name of w",
    "if t is not missing value then set out to out & t & linefeed",
    "end repeat",
    "end repeat",
    "end tell",
    "return out",
];

/// Runs external programs and keeps time.
pub trait Runner {
    /// Runs `program` with `args` to completion within `limit`, returning its output.
    ///
    /// # Errors
    ///
    /// [`Error::Spawn`] for a failed start, [`Error::Exit`] for a failing exit
    /// status, [`Error::TimedOut`] once `limit` passes.
    fn run(
        &mut self,
        program: &str,
        args: &[&str],
        limit: Option<Duration>,
    ) -> Result<Output, Error>;

    /// Waits for `duration`.
    fn sleep(&mut self, duration: Duration);

    /// Monotonic time since the runner was created.
    fn elapsed(&mut self) -> Duration;
}

/// What a finished program printed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// Runs real processes on the system clock.
#[derive(Debug)]
pub struct SystemRunner {
    created: Instant,
}

impl Default for SystemRunner {
    fn default() -> Self {
        Self {
            created: Instant::now(),
        }
    }
}

impl Runner for SystemRunner {
    #[expect(
        clippy::disallowed_methods,
        reason = "SystemRunner is the seam that starts processes"
    )]
    fn run(
        &mut self,
        program: &str,
        args: &[&str],
        limit: Option<Duration>,
    ) -> Result<Output, Error> {
        let command = || command_line(program, args);
        // Its own process group lets a timeout end everything the program started.
        let mut child = process::Command::new(program)
            .args(args)
            .process_group(0)
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .spawn()
            .map_err(|e| Error::Spawn {
                command: command(),
                detail: e.to_string(),
            })?;
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let waited = crate::process::wait_within(&mut child, limit).map_err(|e| Error::Spawn {
            command: command(),
            detail: e.to_string(),
        })?;
        let Some(status) = waited else {
            let group = format!("-{}", child.id());
            let _ = process::Command::new("kill")
                .args(["-KILL", "--", &group])
                .status();
            let _ = child.wait();
            // The pipes close once every process in the group has ended.
            let _ = (stdout.join(), stderr.join());
            return Err(Error::TimedOut { command: command() });
        };
        let output = Output {
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        };
        if status.success() {
            Ok(output)
        } else {
            Err(Error::Exit {
                command: command(),
                code: status.code(),
                stderr: output.stderr.trim().to_owned(),
            })
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "SystemRunner is the seam that really sleeps"
    )]
    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }

    fn elapsed(&mut self) -> Duration {
        self.created.elapsed()
    }
}

/// Why a step failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A program that failed to start.
    Spawn {
        /// The command line.
        command: String,
        /// The I/O error.
        detail: String,
    },
    /// A program exited unsuccessfully.
    Exit {
        /// The command line.
        command: String,
        /// Its exit code, or `None` if a signal ended it.
        code: Option<i32>,
        /// Its stderr output.
        stderr: String,
    },
    /// A step stopped because its recording ended.
    Interrupted,
    /// Text outside the keyboard layout.
    Untypable {
        /// The text.
        text: String,
        /// cliclick's message.
        detail: String,
    },
    /// A program that ran past its time limit and was stopped.
    TimedOut {
        /// The command line.
        command: String,
    },
    /// Program output in an unexpected shape.
    UnexpectedOutput {
        /// What the output describes.
        what: &'static str,
        /// The output.
        output: String,
    },
    /// A `dock:` name missing from the Dock.
    NotInDock(String),
    /// A `WaitWindow` that reached its timeout.
    WindowNotFound(String),
    /// A `WaitUntil` that reached its timeout.
    ConditionNotMet(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { command, detail } => write!(f, "could not run `{command}`: {detail}"),
            Self::Exit {
                command,
                code: Some(code),
                stderr,
            } => write!(f, "`{command}` exited with {code}: {stderr}"),
            Self::Exit {
                command,
                code: None,
                stderr,
            } => write!(f, "`{command}` was killed: {stderr}"),
            Self::UnexpectedOutput { what, output } => {
                write!(f, "could not read {what} from {output:?}")
            }
            Self::Interrupted => write!(f, "the recording ended during the step"),
            Self::Untypable { text, detail } => write!(f, "cannot type {text:?}: {detail}"),
            Self::TimedOut { command } => write!(f, "`{command}` ran past its time limit"),
            Self::NotInDock(name) => write!(f, "{name} is not in the Dock"),
            Self::WindowNotFound(name) => write!(f, "no window matching {name:?} appeared"),
            Self::ConditionNotMet(command) => write!(f, "`{command}` never succeeded"),
        }
    }
}

impl std::error::Error for Error {}

/// Drives the desktop, tracking speed settings between steps.
pub struct Desktop<R: Runner> {
    runner: R,
    typing_delay: Duration,
    move_duration: Duration,
}

impl<R: Runner> Desktop<R> {
    /// A desktop typing at 50 ms per key and gliding in 600 ms.
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            typing_delay: Duration::from_millis(50),
            move_duration: Duration::from_millis(600),
        }
    }

    /// Consumes the desktop, returning its runner.
    pub fn into_runner(self) -> R {
        self.runner
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
            Step::MoveTo(target) => self.glide(target)?,
            Step::Click(target) => {
                self.glide(target)?;
                // `.` is the current position.
                self.cliclick(&["c:."])?;
            }
            Step::WaitWindow { name, timeout } => self.wait_window(name, *timeout, alive)?,
            Step::WaitUntil { command, timeout } => self.wait_until(command, *timeout, alive)?,
            // Warps mark the recording; the caller times them.
            Step::Warp { .. } | Step::EndWarp => {}
            Step::Type(text) => self.type_text(text)?,
            Step::Key(chord) => self.press(chord)?,
        }
        Ok(())
    }

    /// Plays one step.
    ///
    /// # Errors
    ///
    /// Fails if cliclick or osascript fails, or a wait times out.
    pub fn play(&mut self, step: &Step) -> Result<(), Error> {
        self.play_while(step, &mut || true)
    }

    /// Saves the screen to `path` as PNG, silently.
    ///
    /// # Errors
    ///
    /// Fails if screencapture fails.
    pub fn screenshot(&mut self, path: &str) -> Result<(), Error> {
        self.runner
            .run("screencapture", &["-x", path], None)
            .map(drop)
    }

    /// Sleeps for `duration` in slices of at most [`ALIVE_INTERVAL`], checking `alive` before each.
    fn sleep_while(
        &mut self,
        duration: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let start = self.runner.elapsed();
        loop {
            if !alive() {
                return Err(Error::Interrupted);
            }
            let left = duration.saturating_sub(self.runner.elapsed().saturating_sub(start));
            if left.is_zero() {
                return Ok(());
            }
            self.runner.sleep(left.min(ALIVE_INTERVAL));
        }
    }

    /// Sleeps until `offset` after `start` on the runner's clock.
    fn sleep_until(&mut self, start: Duration, offset: Duration) {
        let passed = self.runner.elapsed().saturating_sub(start);
        let remaining = offset.saturating_sub(passed);
        if !remaining.is_zero() {
            self.runner.sleep(remaining);
        }
    }

    fn cliclick(&mut self, commands: &[&str]) -> Result<Output, Error> {
        self.runner.run("cliclick", commands, None)
    }

    /// Types `text` one keystroke per cliclick run, paced by the typing delay.
    fn type_text(&mut self, text: &str) -> Result<(), Error> {
        let start = self.runner.elapsed();
        for (i, c) in (0u32..).zip(text.chars()) {
            // Keystroke `i` starts `i` typing delays in.
            self.sleep_until(start, self.typing_delay.saturating_mul(i));
            let output = match c {
                '\n' => self.cliclick(&["kp:return", KEY_SETTLE])?,
                '\t' => self.cliclick(&["kp:tab", KEY_SETTLE])?,
                c => self.cliclick(&[&format!("t:{c}")])?,
            };
            let detail = output.stderr.trim();
            if detail.contains(UNTYPABLE) {
                return Err(Error::Untypable {
                    text: c.to_string(),
                    detail: detail.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Holds the chord's modifiers, presses its key, and releases them.
    fn press(&mut self, chord: &KeyChord) -> Result<(), Error> {
        let modifiers = chord
            .modifiers()
            .iter()
            .map(|m| m.cliclick_name())
            .collect::<Vec<_>>()
            .join(",");
        let key = match chord.key() {
            Key::Named(name) => format!("kp:{name}"),
            Key::Char(c) => format!("t:{c}"),
        };
        if modifiers.is_empty() {
            self.cliclick(&[&key, KEY_SETTLE])?;
        } else {
            let (down, up) = (format!("kd:{modifiers}"), format!("ku:{modifiers}"));
            self.cliclick(&[&down, &key, &up])?;
        }
        Ok(())
    }

    /// Moves the pointer to `target` along an eased path at [`MOVE_RATE_HZ`].
    fn glide(&mut self, target: &Target) -> Result<(), Error> {
        let to = match target {
            Target::Point(point) => *point,
            Target::Dock(item) => self.dock_point(item)?,
        };
        let from = self.pointer()?;
        let ticks = self.move_duration.as_millis() * u128::from(MOVE_RATE_HZ) / 1000;
        let steps =
            NonZeroU32::new(u32::try_from(ticks).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN);
        let start = self.runner.elapsed();
        for (i, point) in (1..=steps.get()).zip(motion::path(from, to, steps)) {
            // Each move is due at a fixed offset from the glide's start.
            let due = self.move_duration * i / steps.get();
            self.sleep_until(start, due);
            self.cliclick(&[&format!("m:{},{}", point.x, point.y)])?;
        }
        Ok(())
    }

    /// Reads the pointer's `x,y` from `cliclick p`.
    fn pointer(&mut self) -> Result<Point, Error> {
        let output = self.cliclick(&["p"])?.stdout;
        let point = output.trim().split_once(',').and_then(|(x, y)| {
            Some(Point {
                x: units::natural(x)?,
                y: units::natural(y)?,
            })
        });
        point.ok_or(Error::UnexpectedOutput {
            what: "pointer location",
            output,
        })
    }

    /// The centre of `item`'s icon, from the Dock's own geometry.
    fn dock_point(&mut self, item: &DockItem) -> Result<Point, Error> {
        let output = self
            .runner
            .run("osascript", &osascript_args(DOCK_SCRIPT), None)?
            .stdout;
        let items = parse_dock(&output).ok_or(Error::UnexpectedOutput {
            what: "Dock items",
            output,
        })?;
        items
            .into_iter()
            .find_map(|(name, centre)| (name == item.name()).then_some(centre))
            .ok_or_else(|| Error::NotInDock(item.name().to_owned()))
    }

    /// Polls until a foreground window's title contains `name`, ignoring case.
    fn wait_window(
        &mut self,
        name: &WindowName,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let args = osascript_args(WINDOWS_SCRIPT);
        let needle = name.as_str().to_lowercase();
        let found = self.poll(
            "osascript",
            &args,
            POLL_INTERVAL,
            timeout,
            alive,
            |attempt| {
                attempt.map(|output| {
                    output
                        .stdout
                        .lines()
                        .any(|title| title.to_lowercase().contains(&needle))
                })
            },
        )?;
        if found {
            Ok(())
        } else {
            Err(Error::WindowNotFound(name.as_str().to_owned()))
        }
    }

    /// Polls `sh -c <command>` until it exits 0.
    fn wait_until(
        &mut self,
        command: &ShellCommand,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let args = ["-c", command.as_str()];
        // A failing exit status means try again; a failed start ends the wait.
        let found = self.poll(
            "sh",
            &args,
            UNTIL_POLL_INTERVAL,
            timeout,
            alive,
            |attempt| match attempt {
                Ok(_) => Ok(true),
                Err(Error::Exit { .. }) => Ok(false),
                Err(e) => Err(e),
            },
        )?;
        if found {
            Ok(())
        } else {
            Err(Error::ConditionNotMet(command.as_str().to_owned()))
        }
    }

    /// Reruns `program args` every `interval` until `done` accepts an attempt or `timeout` passes.
    fn poll(
        &mut self,
        program: &str,
        args: &[&str],
        interval: Duration,
        timeout: Duration,
        alive: &mut dyn FnMut() -> bool,
        mut done: impl FnMut(Result<Output, Error>) -> Result<bool, Error>,
    ) -> Result<bool, Error> {
        let deadline = self.runner.elapsed().saturating_add(timeout);
        loop {
            if !alive() {
                return Err(Error::Interrupted);
            }
            let left = deadline.saturating_sub(self.runner.elapsed());
            match self.runner.run(program, args, Some(left)) {
                // An attempt cut off at the deadline ends the wait.
                Err(Error::TimedOut { .. }) => return Ok(false),
                attempt => {
                    if done(attempt)? {
                        return Ok(true);
                    }
                }
            }
            let now = self.runner.elapsed();
            if now >= deadline {
                return Ok(false);
            }
            self.runner
                .sleep(interval.min(deadline.saturating_sub(now)));
        }
    }
}

/// `program args…` as one line, for messages and logs.
fn command_line(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads a pipe to its end on its own thread, keeping the program's output flowing.
fn drain(pipe: Option<impl io::Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let mut bytes = Vec::new();
            // A read error leaves whatever arrived before it.
            let _ = pipe.read_to_end(&mut bytes);
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        text
    })
}

/// `osascript` arguments that run `lines` as one script.
fn osascript_args<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines.iter().flat_map(|line| ["-e", line]).collect()
}

/// Parses [`DOCK_SCRIPT`] output into names and icon centres.
fn parse_dock(text: &str) -> Option<Vec<(String, Point)>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?;
            let mut number = || fields.next().and_then(units::natural::<u32>);
            let (x, y, w, h) = (number()?, number()?, number()?, number()?);
            if fields.next().is_some() {
                return None;
            }
            let centre = Point {
                x: x.checked_add(w / 2)?,
                y: y.checked_add(h / 2)?,
            };
            Some((name.to_owned(), centre))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tape;

    /// Records every call and replies from a script of canned outputs.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        replies: Vec<(&'static str, Result<Output, Error>)>,
        slept: Duration,
        /// Virtual time, advanced by `sleep` and by `run_cost` per `run`.
        clock: Duration,
        run_cost: Duration,
    }

    impl Fake {
        fn answer(mut self, prefix: &'static str, stdout: &str, stderr: &str) -> Self {
            let output = Output {
                stdout: stdout.into(),
                stderr: stderr.into(),
            };
            self.replies.push((prefix, Ok(output)));
            self
        }

        fn reply(self, prefix: &'static str, stdout: &str) -> Self {
            self.answer(prefix, stdout, "")
        }

        fn warn(self, prefix: &'static str, stderr: &str) -> Self {
            self.answer(prefix, "", stderr)
        }

        fn exit(mut self, prefix: &'static str, code: i32) -> Self {
            let error = Error::Exit {
                command: prefix.into(),
                code: Some(code),
                stderr: String::new(),
            };
            self.replies.push((prefix, Err(error)));
            self
        }
    }

    impl Runner for Fake {
        fn run(
            &mut self,
            program: &str,
            args: &[&str],
            limit: Option<Duration>,
        ) -> Result<Output, Error> {
            // osascript calls are logged by script name.
            let line = if program == "osascript" {
                let script = if args == osascript_args(DOCK_SCRIPT) {
                    "dock"
                } else if args == osascript_args(WINDOWS_SCRIPT) {
                    "windows"
                } else {
                    "unknown"
                };
                format!("osascript {script}")
            } else {
                command_line(program, args)
            };
            self.calls.push(line.clone());
            if let Some(limit) = limit.filter(|limit| self.run_cost > *limit) {
                self.clock += limit;
                return Err(Error::TimedOut { command: line });
            }
            self.clock += self.run_cost;
            match self
                .replies
                .iter()
                .position(|(prefix, _)| line.starts_with(prefix))
            {
                Some(i) => self.replies.remove(i).1,
                None => Ok(Output::default()),
            }
        }

        fn sleep(&mut self, duration: Duration) {
            self.slept += duration;
            self.clock += duration;
        }

        fn elapsed(&mut self) -> Duration {
            self.clock
        }
    }

    /// Steps built through the parser.
    fn steps(body: &str) -> Vec<Step> {
        tape::parse(&format!("Output t.mp4\n{body}")).unwrap().steps
    }

    fn play(fake: Fake, body: &str) -> Result<Fake, Error> {
        let mut desktop = Desktop::new(fake);
        for step in &steps(body) {
            desktop.play(step)?;
        }
        Ok(desktop.into_runner())
    }

    fn moves(fake: &Fake) -> Vec<&str> {
        fake.calls
            .iter()
            .filter(|c| c.starts_with("cliclick m:"))
            .map(String::as_str)
            .collect()
    }

    const POINTER: &str = "cliclick p";
    const DOCK: &str = "osascript dock";
    const WINDOWS: &str = "osascript windows";
    /// Dock geometry as reported on a macos-26 runner.
    const DOCK_ITEMS: &str = "Finder\t196\t986\t68\t84\nSystem Settings\t1420\t986\t68\t84\n\
                              Terminal\t1488\t986\t68\t84\nmissing value\t1556\t986\t30\t84\n";

    #[test]
    fn sleep_waits() {
        let fake = play(Fake::default(), "Sleep 1.5s").unwrap();
        assert_eq!(fake.slept, Duration::from_millis(1500));
        assert_eq!(fake.calls, Vec::<String>::new());
    }

    #[test]
    fn types_one_keystroke_at_a_time_at_typing_speed() {
        let fake = play(Fake::default(), "Set TypingSpeed 80ms\nType \"hi!\"").unwrap();
        assert_eq!(fake.calls, ["cliclick t:h", "cliclick t:i", "cliclick t:!"]);
        // Pauses go between keystrokes.
        assert_eq!(fake.slept, Duration::from_millis(160));
    }

    #[test]
    fn untypable_characters_fail_the_step() {
        let fake = Fake::default().warn(
            "cliclick t:é",
            "Unable to get key code for é (see https://github.com/BlueM/cliclick)\n",
        );
        let err = play(fake, "Type \"café\"").err();
        assert!(
            matches!(&err, Some(Error::Untypable { text, .. }) if text == "é"),
            "{err:?}"
        );
    }

    #[test]
    fn other_cliclick_warnings_keep_typing() {
        let fake = Fake::default().warn("cliclick t:a", "cliclick: some unrelated notice\n");
        assert!(play(fake, "Type \"ab\"").is_ok());
    }

    #[test]
    fn types_newline_and_tab_as_keys() {
        let fake = play(Fake::default(), "Type \"a\\tb\\n\"").unwrap();
        assert_eq!(
            fake.calls,
            [
                "cliclick t:a",
                "cliclick kp:tab w:50",
                "cliclick t:b",
                "cliclick kp:return w:50"
            ]
        );
    }

    #[test]
    fn presses_chords_with_modifiers_held() {
        let fake = play(
            Fake::default(),
            "Key return\nKey cmd+space\nKey cmd+shift+p",
        )
        .unwrap();
        assert_eq!(
            fake.calls,
            [
                "cliclick kp:return w:50",
                "cliclick kd:cmd kp:space ku:cmd",
                "cliclick kd:cmd,shift t:p ku:cmd,shift",
            ]
        );
    }

    #[test]
    fn glide_time_includes_the_moves_themselves() {
        let fake = Fake {
            run_cost: Duration::from_millis(10),
            ..Fake::default()
        }
        .reply(POINTER, "0,0\n");
        let fake = play(fake, "Set MoveDuration 500ms\nMoveTo 900,600").unwrap();
        // Pointer read, 500 ms of glide, and the last move.
        assert!(fake.clock <= Duration::from_millis(520), "{:?}", fake.clock);
        assert_eq!(moves(&fake).len(), 30);
    }

    #[test]
    fn keystrokes_start_typing_speed_apart() {
        let fake = Fake {
            run_cost: Duration::from_millis(10),
            ..Fake::default()
        };
        let fake = play(fake, "Set TypingSpeed 80ms\nType \"hi!\"").unwrap();
        // Keystrokes start at 0, 80 and 160 ms; the last one takes 10 ms.
        assert_eq!(fake.clock, Duration::from_millis(170));
    }

    #[test]
    fn move_glides_at_sixty_hz_from_current_position() {
        let fake = Fake::default().reply(POINTER, "100,200\n");
        let fake = play(fake, "Set MoveDuration 500ms\nMoveTo 900,600").unwrap();
        let moves = moves(&fake);
        assert_eq!(moves.len(), 30);
        assert_eq!(moves.last(), Some(&"cliclick m:900,600"));
        assert_eq!(fake.slept, Duration::from_millis(500));
    }

    #[test]
    fn click_glides_then_clicks_in_place() {
        let fake = Fake::default().reply(POINTER, "0,0\n");
        let fake = play(fake, "Click 10,20").unwrap();
        assert_eq!(moves(&fake).last(), Some(&"cliclick m:10,20"));
        assert_eq!(fake.calls.last().map(String::as_str), Some("cliclick c:."));
    }

    #[test]
    fn dock_targets_aim_at_the_icon_centre() {
        let fake = Fake::default()
            .reply(POINTER, "0,0\n")
            .reply(DOCK, DOCK_ITEMS);
        let fake = play(fake, "MoveTo dock:Terminal").unwrap();
        // The icon centre: 1488 + 68/2, 986 + 84/2.
        assert_eq!(moves(&fake).last(), Some(&"cliclick m:1522,1028"));

        let fake = Fake::default()
            .reply(POINTER, "0,0\n")
            .reply(DOCK, DOCK_ITEMS);
        let fake = play(fake, "MoveTo dock:\"System Settings\"").unwrap();
        assert_eq!(moves(&fake).last(), Some(&"cliclick m:1454,1028"));
    }

    #[test]
    fn dock_targets_must_be_in_the_dock() {
        let fake = Fake::default()
            .reply(POINTER, "0,0\n")
            .reply(DOCK, DOCK_ITEMS);
        let err = play(fake, "Click dock:Safari").err();
        assert_eq!(err, Some(Error::NotInDock("Safari".into())));
    }

    #[test]
    fn garbled_dock_listing_is_an_error() {
        let fake = Fake::default()
            .reply(POINTER, "0,0\n")
            .reply(DOCK, "Terminal\t1488\t986\n");
        let err = play(fake, "Click dock:Terminal").err();
        assert!(
            matches!(
                err,
                Some(Error::UnexpectedOutput {
                    what: "Dock items",
                    ..
                })
            ),
            "{err:?}"
        );
    }

    /// Dock script output captured on a macos-26 runner, osascript's own newline included.
    const RUNNER_DOCK: &str = "Finder\t196\t986\t68\t84\nApps\t264\t986\t68\t84\n\
        System Settings\t1420\t986\t68\t84\nTerminal\t1488\t986\t68\t84\n\
        missing value\t1556\t986\t30\t84\nDownloads\t1587\t986\t68\t84\nTrash\t1655\t986\t68\t84\n\n";

    #[test]
    fn parses_dock_listings_as_osascript_prints_them() {
        let items = parse_dock(RUNNER_DOCK).unwrap();
        assert_eq!(items.len(), 7, "{items:?}");
        assert_eq!(
            items[3],
            ("Terminal".to_owned(), Point { x: 1522, y: 1028 })
        );
    }

    #[test]
    fn parses_dock_listings() {
        let items = parse_dock(DOCK_ITEMS).unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[2],
            ("Terminal".to_owned(), Point { x: 1522, y: 1028 })
        );
        assert_eq!(parse_dock(""), Some(vec![]));
        for bad in [
            "a\t1\t2\t3",
            "a\t1\t2\t3\tx",
            "a\t-1\t2\t3\t4",
            "a\t+1\t2\t3\t4",
            "a\t1.5\t2\t3\t4",
        ] {
            assert_eq!(parse_dock(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn wait_window_polls_until_a_title_contains_the_text() {
        let fake = Fake::default()
            .reply(WINDOWS, "Finder\n")
            .reply(WINDOWS, "Finder\n")
            .reply(WINDOWS, "Finder\nrunner — -bash — 80×24\n");
        let fake = play(fake, "WaitWindow \"bash\"").unwrap();
        assert_eq!(fake.calls, ["osascript windows"; 3]);
        assert_eq!(fake.slept, POLL_INTERVAL * 2);
    }

    #[test]
    fn wait_window_ignores_case() {
        let fake = Fake::default().reply(WINDOWS, "Finder\no3s - Visual Studio Code\n");
        let fake = play(fake, "WaitWindow \"VISUAL studio\"").unwrap();
        assert_eq!(fake.calls, ["osascript windows"]);
    }

    #[test]
    fn screenshot_captures_silently() {
        let mut desktop = Desktop::new(Fake::default());
        desktop.screenshot("/out/fail.png").unwrap();
        assert_eq!(
            desktop.into_runner().calls,
            ["screencapture -x /out/fail.png"]
        );
    }

    #[test]
    fn wait_window_respects_timeout() {
        let mut fake = Fake::default();
        for _ in 0..1000 {
            fake = fake.reply(WINDOWS, "Finder\n");
        }
        let mut desktop = Desktop::new(fake);
        let err = steps("WaitWindow \"Nope\" 1s")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(err, Some(Error::WindowNotFound("Nope".into())));
        let fake = desktop.into_runner();
        assert_eq!(fake.calls.len(), 11);
        assert_eq!(fake.slept, Duration::from_secs(1));
    }

    #[test]
    fn wait_window_stops_on_osascript_failure() {
        let fake = Fake::default().exit(WINDOWS, 1);
        let err = play(fake, "WaitWindow \"Terminal\"").err();
        assert!(
            matches!(err, Some(Error::Exit { code: Some(1), .. })),
            "{err:?}"
        );
    }

    #[test]
    fn wait_until_polls_shell_until_success() {
        let fake = Fake::default()
            .exit("sh -c", 1)
            .exit("sh -c", 127)
            .reply("sh -c", "");
        let fake = play(fake, "WaitUntil \"command -v code\" 1m").unwrap();
        assert_eq!(fake.calls, ["sh -c command -v code"; 3]);
        assert_eq!(fake.slept, UNTIL_POLL_INTERVAL * 2);
    }

    #[test]
    fn wait_until_timeout_counts_time_spent_running() {
        let mut fake = Fake {
            run_cost: Duration::from_secs(10),
            ..Fake::default()
        };
        for _ in 0..1000 {
            fake = fake.exit("sh -c", 1);
        }
        let mut desktop = Desktop::new(fake);
        let err = steps("WaitUntil \"slow-check\" 1m")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(err, Some(Error::ConditionNotMet("slow-check".into())));
        let fake = desktop.into_runner();
        // Ends within one run and one poll interval of the deadline.
        assert!(
            fake.clock <= Duration::from_secs(70) + UNTIL_POLL_INTERVAL,
            "{:?}",
            fake.clock
        );
        assert!(fake.calls.len() <= 7, "{} runs", fake.calls.len());
    }

    #[test]
    fn wait_until_ends_a_hanging_attempt_at_the_deadline() {
        let fake = Fake {
            run_cost: Duration::from_secs(600),
            ..Fake::default()
        };
        let mut desktop = Desktop::new(fake);
        let err = steps("WaitUntil \"curl -s http://stuck\" 1m")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(
            err,
            Some(Error::ConditionNotMet("curl -s http://stuck".into()))
        );
        assert_eq!(desktop.into_runner().clock, Duration::from_secs(60));
    }

    /// An `alive` check that turns false on its `n`th call.
    fn alive_for(n: usize) -> impl FnMut() -> bool {
        let mut calls = 0;
        move || {
            calls += 1;
            calls < n
        }
    }

    #[test]
    fn waits_end_when_the_recording_stops() {
        let mut fake = Fake::default();
        for _ in 0..1000 {
            fake = fake.exit("sh -c", 1);
        }
        let mut desktop = Desktop::new(fake);
        let step = &steps("WaitUntil \"false\" 30m")[0];
        assert_eq!(
            desktop.play_while(step, &mut alive_for(3)),
            Err(Error::Interrupted)
        );
        assert!(desktop.into_runner().calls.len() <= 3);
    }

    #[test]
    fn sleeps_end_when_the_recording_stops() {
        let mut desktop = Desktop::new(Fake::default());
        let step = &steps("Sleep 5m")[0];
        assert_eq!(
            desktop.play_while(step, &mut alive_for(3)),
            Err(Error::Interrupted)
        );
        assert!(desktop.into_runner().slept <= Duration::from_secs(3));
    }

    #[test]
    fn wait_until_times_out() {
        let mut fake = Fake::default();
        for _ in 0..1000 {
            fake = fake.exit("sh -c", 1);
        }
        let err = play(fake, "WaitUntil \"false\" 2s").err();
        assert_eq!(err, Some(Error::ConditionNotMet("false".into())));
    }

    #[test]
    fn warp_markers_touch_nothing() {
        let fake = play(Fake::default(), "Warp \"x\" 1s\nEndWarp").unwrap();
        assert_eq!(fake.calls, Vec::<String>::new());
        assert_eq!(fake.slept, Duration::ZERO);
    }

    #[test]
    fn garbled_pointer_location_is_an_error() {
        for output in ["huh\n", "1,2,3\n", "-5,7\n", "+5,7\n"] {
            let fake = Fake::default().reply(POINTER, output);
            let err = play(fake, "MoveTo 1,1").err();
            assert!(
                matches!(
                    err,
                    Some(Error::UnexpectedOutput {
                        what: "pointer location",
                        ..
                    })
                ),
                "{output:?}: {err:?}"
            );
        }
    }

    #[test]
    fn system_runner_kills_the_whole_process_group_at_the_limit() {
        let mut runner = SystemRunner::default();
        let started = Instant::now();
        // The background `sleep` holds the output pipe until it dies too.
        let result = runner.run(
            "sh",
            &["-c", "sleep 30 & sleep 30"],
            Some(Duration::from_millis(200)),
        );
        assert!(matches!(result, Err(Error::TimedOut { .. })), "{result:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn system_runner_reports_spawn_and_exit_failures() {
        let mut runner = SystemRunner::default();
        assert_eq!(runner.run("echo", &["hi"], None).unwrap().stdout, "hi\n");
        assert!(matches!(
            runner.run("/nonexistent/program", &[], None),
            Err(Error::Spawn { .. })
        ));
        assert!(matches!(
            runner.run("false", &[], None),
            Err(Error::Exit { code: Some(1), .. })
        ));
    }
}
