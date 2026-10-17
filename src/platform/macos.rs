//! macOS through the `taperec-desktop` helper, `sh` and `screencapture`.

use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::{CaptureError, Control, Error, Platform, Recorder};
use crate::process::{Output, Runner};
use crate::recorder::Recording;
use crate::tape::{AppName, KeyChord, Point, ShellCommand};
use crate::units;

/// The Swift helper that drives input and reads the screen (`tools/desktop`).
const HELPER: &str = "taperec-desktop";
/// What the helper prints once the first frame is written.
const RECORDING: &str = "recording";
/// How long screen capture may take to start.
const RECORDING_LIMIT: Duration = Duration::from_secs(30);
/// The helper's exit code for a character outside the keyboard layout.
const UNTYPABLE: i32 = 3;

/// macOS driven by the desktop helper and system tools through a [`Runner`].
pub struct Helper<R: Runner> {
    runner: R,
}

impl<R: Runner> Helper<R> {
    /// macOS through `runner`.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Consumes the platform, returning its runner.
    pub fn into_runner(self) -> R {
        self.runner
    }

    fn helper(&mut self, args: &[&str], limit: Option<Duration>) -> Result<Output, Error> {
        self.runner.run(HELPER, args, limit)
    }
}

impl<R: Runner> Platform for Helper<R> {
    fn sleep(&mut self, duration: Duration) {
        self.runner.sleep(duration);
    }

    fn elapsed(&mut self) -> Duration {
        self.runner.elapsed()
    }

    /// Reads the pointer's `x<TAB>y`.
    fn pointer(&mut self) -> Result<Point, Error> {
        let output = self.helper(&["pointer"], None)?.stdout;
        let point = output.trim_end().split_once('\t').and_then(|(x, y)| {
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

    fn move_to(&mut self, point: Point) -> Result<(), Error> {
        let (x, y) = (point.x.to_string(), point.y.to_string());
        self.helper(&["move", &x, &y], None).map(drop)
    }

    fn click(&mut self) -> Result<(), Error> {
        self.helper(&["click"], None).map(drop)
    }

    fn key(&mut self, chord: &KeyChord) -> Result<(), Error> {
        self.helper(&["key", &chord.to_string()], None).map(drop)
    }

    fn type_char(&mut self, c: char) -> Result<(), Error> {
        match self.helper(&["type", c.encode_utf8(&mut [0; 4])], None) {
            Err(Error::Exit {
                code: Some(UNTYPABLE),
                stderr,
                ..
            }) => Err(Error::Untypable {
                text: c.to_string(),
                detail: stderr.trim().to_owned(),
            }),
            result => result.map(drop),
        }
    }

    fn paste(&mut self, text: &str) -> Result<(), Error> {
        self.helper(&["paste", text], None).map(drop)
    }

    fn windows(&mut self, limit: Option<Duration>) -> Result<Vec<String>, Error> {
        let output = self.helper(&["windows"], limit)?;
        Ok(output.stdout.lines().map(str::to_owned).collect())
    }

    fn buttons(&mut self, limit: Option<Duration>) -> Result<Vec<Control>, Error> {
        let output = self.helper(&["buttons"], limit)?.stdout;
        parse_controls(&output).ok_or(Error::UnexpectedOutput {
            what: "on-screen buttons",
            output,
        })
    }

    fn dock(&mut self) -> Result<Vec<(String, Point)>, Error> {
        let output = self.helper(&["dock"], None)?.stdout;
        parse_dock(&output).ok_or(Error::UnexpectedOutput {
            what: "Dock items",
            output,
        })
    }

    fn focus(&mut self, app: &AppName, limit: Option<Duration>) -> Result<bool, Error> {
        let output = self.helper(&["focus", app.as_str()], limit)?;
        Ok(output.stdout.trim() == "true")
    }

    /// Runs `sh -c <command>`; a failing exit status is `false`, a failed start an error.
    fn shell(&mut self, command: &ShellCommand, limit: Option<Duration>) -> Result<bool, Error> {
        match self.runner.run("sh", &["-c", command.as_str()], limit) {
            Ok(_) => Ok(true),
            Err(Error::Exit { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn screenshot(&mut self, path: &Path) -> Result<(), Error> {
        let path = path.to_string_lossy();
        self.runner
            .run("screencapture", &["-x", &path], None)
            .map(drop)
    }
}

/// The screen captured by the helper through `ScreenCaptureKit`, its frames stamped with wall-clock time.
#[derive(Default)]
pub struct ScreenRecorder {
    recording: Option<Recording>,
}

impl Recorder for ScreenRecorder {
    fn start(&mut self, out: &Path) -> Result<(), CaptureError> {
        let mut recording = Recording::start(HELPER, &record_args(out))?;
        recording.wait_for(RECORDING, RECORDING_LIMIT)?;
        self.recording = Some(recording);
        Ok(())
    }

    fn check(&mut self) -> Result<(), CaptureError> {
        self.recording.as_mut().map_or(Ok(()), Recording::check)
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.recording.take().map_or(Ok(()), Recording::stop)
    }

    fn abort(&mut self) {
        if let Some(recording) = self.recording.take() {
            recording.abort();
        }
    }

    /// Seconds since the Unix epoch, the clock the helper stamps frames with.
    fn clock(&self) -> Duration {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
    }
}

/// Helper arguments that record the screen into `out`.
fn record_args(out: &Path) -> [OsString; 2] {
    [OsString::from("record"), out.as_os_str().to_owned()]
}

/// Parses `buttons` output.
fn parse_controls(text: &str) -> Option<Vec<Control>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let app = fields.next()?.to_owned();
            let frontmost = match fields.next()? {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            let label = fields.next()?.to_owned();
            let mut number = || fields.next().and_then(units::natural::<u32>);
            let (x, y, w, h) = (number()?, number()?, number()?, number()?);
            if fields.next().is_some() {
                return None;
            }
            let centre = Point {
                x: x.checked_add(w / 2)?,
                y: y.checked_add(h / 2)?,
            };
            Some(Control {
                app,
                frontmost,
                label,
                centre,
            })
        })
        .collect()
}

/// Parses `dock` output into names and icon centres.
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
    use crate::desktop::{Desktop, POLL_INTERVAL, UNTIL_POLL_INTERVAL};
    use crate::process::command_line;
    use crate::tape::{self, Step};

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
            let line = command_line(program, args);
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
        tape::parse(&format!("output: t.mp4\nsteps:\n{body}"))
            .unwrap()
            .steps
    }

    fn play(fake: Fake, body: &str) -> Result<Fake, Error> {
        let mut desktop = Desktop::new(Helper::new(fake));
        for step in &steps(body) {
            desktop.play(step)?;
        }
        Ok(desktop.into_platform().into_runner())
    }

    fn moves(fake: &Fake) -> Vec<&str> {
        fake.calls
            .iter()
            .filter(|c| c.starts_with("taperec-desktop move "))
            .map(String::as_str)
            .collect()
    }

    const POINTER: &str = "taperec-desktop pointer";
    const DOCK: &str = "taperec-desktop dock";
    const WINDOWS: &str = "taperec-desktop windows";
    /// Dock geometry as reported on a macos-26 runner.
    const DOCK_ITEMS: &str = "Finder\t196\t986\t68\t84\nSystem Settings\t1420\t986\t68\t84\n\
                              Terminal\t1488\t986\t68\t84\nmissing value\t1556\t986\t30\t84\n";

    #[test]
    fn records_through_the_helper() {
        assert_eq!(
            record_args(Path::new("/out/demo.raw.mov")),
            [
                OsString::from("record"),
                OsString::from("/out/demo.raw.mov")
            ]
        );
    }

    #[test]
    fn sleep_waits() {
        let fake = play(Fake::default(), "  - sleep: 1.5s\n").unwrap();
        assert_eq!(fake.slept, Duration::from_millis(1500));
        assert_eq!(fake.calls, Vec::<String>::new());
    }

    #[test]
    fn types_one_keystroke_at_a_time_at_typing_speed() {
        let fake = play(
            Fake::default(),
            "  - set: { typingSpeed: 80ms }\n  - type: \"hi!\"\n",
        )
        .unwrap();
        assert_eq!(
            fake.calls,
            [
                "taperec-desktop type h",
                "taperec-desktop type i",
                "taperec-desktop type !"
            ]
        );
        // Pauses go between keystrokes.
        assert_eq!(fake.slept, Duration::from_millis(160));
    }

    #[test]
    fn untypable_characters_fail_the_step() {
        let mut fake = Fake::default();
        fake.replies.push((
            "taperec-desktop type é",
            Err(Error::Exit {
                command: "taperec-desktop type é".into(),
                code: Some(UNTYPABLE),
                stderr: "cannot type \"é\"\n".into(),
            }),
        ));
        let err = play(fake, "  - type: \"café\"\n").err();
        assert_eq!(
            err,
            Some(Error::Untypable {
                text: "é".into(),
                detail: "cannot type \"é\"".into(),
            })
        );
    }

    #[test]
    fn other_helper_failures_stay_failures() {
        let fake = Fake::default().exit("taperec-desktop type a", 2);
        let err = play(fake, "  - type: \"ab\"\n").err();
        assert!(
            matches!(err, Some(Error::Exit { code: Some(2), .. })),
            "{err:?}"
        );
    }

    #[test]
    fn helper_warnings_keep_typing() {
        let fake = Fake::default().warn("taperec-desktop type a", "some unrelated notice\n");
        assert!(play(fake, "  - type: \"ab\"\n").is_ok());
    }

    #[test]
    fn types_newline_and_tab_as_keys() {
        let fake = play(Fake::default(), "  - type: \"a\\tb\\n\"\n").unwrap();
        assert_eq!(
            fake.calls,
            [
                "taperec-desktop type a",
                "taperec-desktop type \t",
                "taperec-desktop type b",
                "taperec-desktop type \n"
            ]
        );
    }

    #[test]
    fn presses_chords_as_tapes_write_them() {
        let fake = play(
            Fake::default(),
            "  - key: \"return\"\n  - key: \"cmd+space\"\n  - key: \"primary+shift+p\"\n",
        )
        .unwrap();
        assert_eq!(
            fake.calls,
            [
                "taperec-desktop key return",
                "taperec-desktop key cmd+space",
                "taperec-desktop key primary+shift+p",
            ]
        );
    }

    #[test]
    fn glide_time_includes_the_moves_themselves() {
        let fake = Fake {
            run_cost: Duration::from_millis(10),
            ..Fake::default()
        }
        .reply(POINTER, "0\t0\n");
        let fake = play(
            fake,
            "  - set: { moveDuration: 500ms }\n  - moveTo: { point: { x: 900, y: 600 } }\n",
        )
        .unwrap();
        // Pointer read, 500 ms of glide, and the last move.
        assert!(fake.clock <= Duration::from_millis(520), "{:?}", fake.clock);
        assert_eq!(moves(&fake).len(), 30);
    }

    #[test]
    fn slow_moves_skip_points_to_keep_the_glide_on_time() {
        let fake = Fake {
            run_cost: Duration::from_millis(100),
            ..Fake::default()
        }
        .reply(POINTER, "0\t0\n");
        let fake = play(
            fake,
            "  - set: { moveDuration: 700ms }\n  - moveTo: { point: { x: 900, y: 600 } }\n",
        )
        .unwrap();
        // The pointer read, the glide, and a move started just before its time running past it.
        assert!(
            fake.clock <= Duration::from_millis(1000),
            "{:?}",
            fake.clock
        );
        assert!(moves(&fake).len() <= 8, "{} moves", moves(&fake).len());
        assert_eq!(moves(&fake).last(), Some(&"taperec-desktop move 900 600"));
    }

    #[test]
    fn keystrokes_start_typing_speed_apart() {
        let fake = Fake {
            run_cost: Duration::from_millis(10),
            ..Fake::default()
        };
        let fake = play(fake, "  - set: { typingSpeed: 80ms }\n  - type: \"hi!\"\n").unwrap();
        // Keystrokes start at 0, 80 and 160 ms; the last one takes 10 ms.
        assert_eq!(fake.clock, Duration::from_millis(170));
    }

    #[test]
    fn move_glides_at_sixty_hz_from_current_position() {
        let fake = Fake::default().reply(POINTER, "100\t200\n");
        let fake = play(
            fake,
            "  - set: { moveDuration: 500ms }\n  - moveTo: { point: { x: 900, y: 600 } }\n",
        )
        .unwrap();
        let moves = moves(&fake);
        assert_eq!(moves.len(), 30);
        assert_eq!(moves.last(), Some(&"taperec-desktop move 900 600"));
        assert_eq!(fake.slept, Duration::from_millis(500));
    }

    #[test]
    fn click_glides_then_clicks_in_place() {
        let fake = Fake::default().reply(POINTER, "0\t0\n");
        let fake = play(fake, "  - click: { point: { x: 10, y: 20 } }\n").unwrap();
        assert_eq!(moves(&fake).last(), Some(&"taperec-desktop move 10 20"));
        assert_eq!(
            fake.calls.last().map(String::as_str),
            Some("taperec-desktop click")
        );
    }

    #[test]
    fn dock_targets_aim_at_the_icon_centre() {
        let fake = Fake::default()
            .reply(POINTER, "0\t0\n")
            .reply(DOCK, DOCK_ITEMS);
        let fake = play(fake, "  - moveTo: { dock: Terminal }\n").unwrap();
        // The icon centre: 1488 + 68/2, 986 + 84/2.
        assert_eq!(moves(&fake).last(), Some(&"taperec-desktop move 1522 1028"));

        let fake = Fake::default()
            .reply(POINTER, "0\t0\n")
            .reply(DOCK, DOCK_ITEMS);
        let fake = play(fake, "  - moveTo: { dock: \"System Settings\" }\n").unwrap();
        assert_eq!(moves(&fake).last(), Some(&"taperec-desktop move 1454 1028"));
    }

    const CONTROLS: &str = "taperec-desktop buttons";
    const FOCUS: &str = "taperec-desktop focus";
    /// The first-launch prompt as accessibility reported it on a macos-26-intel runner.
    const PROMPT: &str = "CoreServicesUIAgent\tfalse\tCancel\t845\t432\t112\t30\n\
                          CoreServicesUIAgent\tfalse\tOpen\t963\t432\t112\t30\n";

    #[test]
    fn buttons_are_clicked_at_their_centre_once_they_appear() {
        let fake = Fake::default()
            .reply(POINTER, "0\t0\n")
            .reply(CONTROLS, "")
            .reply(CONTROLS, PROMPT);
        let fake = play(fake, "  - click: { button: \"Open\", timeout: 5s }\n").unwrap();
        assert_eq!(moves(&fake).last(), Some(&"taperec-desktop move 1019 447"));
        assert_eq!(
            fake.calls.last().map(String::as_str),
            Some("taperec-desktop click")
        );
        let polls = fake.calls.iter().filter(|c| *c == CONTROLS).count();
        assert_eq!(polls, 2);
    }

    #[test]
    fn the_frontmost_app_wins_a_shared_label() {
        let controls =
            "Docker Desktop\tfalse\tOpen\t0\t0\t10\t10\nCode\ttrue\tOpen\t100\t100\t10\t10\n";
        let fake = Fake::default()
            .reply(POINTER, "0\t0\n")
            .reply(CONTROLS, controls);
        let fake = play(fake, "  - moveTo: { button: \"Open\" }\n").unwrap();
        assert_eq!(moves(&fake).last(), Some(&"taperec-desktop move 105 105"));
    }

    #[test]
    fn a_label_in_several_background_apps_is_ambiguous() {
        let controls =
            "Docker Desktop\tfalse\tOpen\t0\t0\t10\t10\nCode\tfalse\tOpen\t100\t100\t10\t10\n";
        let fake = Fake::default().reply(CONTROLS, controls);
        let err = play(fake, "  - click: { button: \"Open\" }\n").err();
        assert_eq!(
            err,
            Some(Error::AmbiguousControl {
                label: "Open".into(),
                apps: vec!["Code".into(), "Docker Desktop".into()],
            })
        );
    }

    #[test]
    fn a_button_that_never_appears_times_out() {
        let fake = Fake::default().reply(CONTROLS, PROMPT);
        let err = play(fake, "  - click: { button: \"Allow\", timeout: 1s }\n").err();
        assert_eq!(err, Some(Error::ControlNotFound("Allow".into())));
    }

    #[test]
    fn garbled_button_listing_is_an_error() {
        let fake = Fake::default().reply(CONTROLS, "Code\tmaybe\tOpen\t1\t2\t3\t4\n");
        let err = play(fake, "  - click: { button: \"Open\" }\n").err();
        assert!(
            matches!(
                err,
                Some(Error::UnexpectedOutput {
                    what: "on-screen buttons",
                    ..
                })
            ),
            "{err:?}"
        );
    }

    #[test]
    fn paste_hands_the_text_to_the_helper() {
        let fake = play(
            Fake::default(),
            "  - paste: \"curl -fsSL https://example.com\"\n",
        )
        .unwrap();
        assert_eq!(
            fake.calls,
            ["taperec-desktop paste curl -fsSL https://example.com"]
        );
    }

    #[test]
    fn focus_waits_until_the_app_is_in_front() {
        let fake = Fake::default()
            .reply(FOCUS, "false\n")
            .reply(FOCUS, "true\n");
        let fake = play(fake, "  - focus: \"Visual Studio Code\"\n").unwrap();
        assert_eq!(fake.calls, ["taperec-desktop focus Visual Studio Code"; 2]);
    }

    #[test]
    fn focus_fails_when_the_app_stays_behind() {
        let err = play(Fake::default(), "  - focus: \"Terminal\"\n").err();
        assert_eq!(err, Some(Error::NotFrontmost("Terminal".into())));
    }

    #[test]
    fn parses_button_listings() {
        let controls = parse_controls(PROMPT).unwrap();
        assert_eq!(controls.len(), 2);
        assert_eq!(controls[1].label, "Open");
        assert_eq!(controls[1].centre, Point { x: 1019, y: 447 });
        assert!(!controls[1].frontmost);
        assert_eq!(parse_controls(""), Some(vec![]));
        assert_eq!(parse_controls("a\ttrue\tb\t1\t2\t3"), None);
    }

    #[test]
    fn dock_targets_must_be_in_the_dock() {
        let fake = Fake::default()
            .reply(POINTER, "0\t0\n")
            .reply(DOCK, DOCK_ITEMS);
        let err = play(fake, "  - click: { dock: Safari }\n").err();
        assert_eq!(err, Some(Error::NotInDock("Safari".into())));
    }

    #[test]
    fn garbled_dock_listing_is_an_error() {
        let fake = Fake::default()
            .reply(POINTER, "0\t0\n")
            .reply(DOCK, "Terminal\t1488\t986\n");
        let err = play(fake, "  - click: { dock: Terminal }\n").err();
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

    /// Dock items from a macos-26 runner, with a trailing blank line.
    const RUNNER_DOCK: &str = "Finder\t196\t986\t68\t84\nApps\t264\t986\t68\t84\n\
        System Settings\t1420\t986\t68\t84\nTerminal\t1488\t986\t68\t84\n\
        missing value\t1556\t986\t30\t84\nDownloads\t1587\t986\t68\t84\nTrash\t1655\t986\t68\t84\n\n";

    #[test]
    fn parses_dock_listings_with_blank_lines() {
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
        let fake = play(fake, "  - waitWindow: { title: \"bash\" }\n").unwrap();
        assert_eq!(fake.calls, [WINDOWS; 3]);
        assert_eq!(fake.slept, POLL_INTERVAL * 2);
    }

    #[test]
    fn wait_window_ignores_case() {
        let fake = Fake::default().reply(WINDOWS, "Finder\no3s - Visual Studio Code\n");
        let fake = play(fake, "  - waitWindow: { title: \"VISUAL studio\" }\n").unwrap();
        assert_eq!(fake.calls, [WINDOWS]);
    }

    #[test]
    fn screenshot_captures_silently() {
        let mut desktop = Desktop::new(Helper::new(Fake::default()));
        desktop.screenshot("/out/fail.png").unwrap();
        assert_eq!(
            desktop.into_platform().into_runner().calls,
            ["screencapture -x /out/fail.png"]
        );
    }

    #[test]
    fn wait_window_respects_timeout() {
        let mut fake = Fake::default();
        for _ in 0..1000 {
            fake = fake.reply(WINDOWS, "Finder\n");
        }
        let mut desktop = Desktop::new(Helper::new(fake));
        let err = steps("  - waitWindow: { title: \"Nope\", timeout: 1s }\n")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(err, Some(Error::WindowNotFound("Nope".into())));
        let fake = desktop.into_platform().into_runner();
        assert_eq!(fake.calls.len(), 11);
        assert_eq!(fake.slept, Duration::from_secs(1));
    }

    #[test]
    fn wait_window_stops_on_helper_failure() {
        let fake = Fake::default().exit(WINDOWS, 1);
        let err = play(fake, "  - waitWindow: { title: \"Terminal\" }\n").err();
        assert!(
            matches!(err, Some(Error::Exit { code: Some(1), .. })),
            "{err:?}"
        );
    }

    #[test]
    fn wait_until_runs_the_failure_check_after_a_failed_attempt() {
        let fake = Fake::default()
            .exit("sh -c test -f ok", 1)
            .reply("sh -c test -f broken", "");
        let mut desktop = Desktop::new(Helper::new(fake));
        let err = steps("  - waitUntil: { command: test -f ok, failWhen: test -f broken }\n")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert!(matches!(err, Some(Error::WaitFailed { .. })), "{err:?}");
        assert_eq!(
            desktop.into_platform().into_runner().calls,
            ["sh -c test -f ok", "sh -c test -f broken"]
        );
    }

    #[test]
    fn wait_until_polls_shell_until_success() {
        let fake = Fake::default()
            .exit("sh -c", 1)
            .exit("sh -c", 127)
            .reply("sh -c", "");
        let fake = play(
            fake,
            "  - waitUntil: { command: \"command -v code\", timeout: 1m }\n",
        )
        .unwrap();
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
        let mut desktop = Desktop::new(Helper::new(fake));
        let err = steps("  - waitUntil: { command: \"slow-check\", timeout: 1m }\n")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(err, Some(Error::ConditionNotMet("slow-check".into())));
        let fake = desktop.into_platform().into_runner();
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
        let mut desktop = Desktop::new(Helper::new(fake));
        let err = steps("  - waitUntil: { command: \"curl -s http://stuck\", timeout: 1m }\n")
            .iter()
            .try_for_each(|s| desktop.play(s))
            .err();
        assert_eq!(
            err,
            Some(Error::ConditionNotMet("curl -s http://stuck".into()))
        );
        assert_eq!(
            desktop.into_platform().into_runner().clock,
            Duration::from_secs(60)
        );
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
        let mut desktop = Desktop::new(Helper::new(fake));
        let step = &steps("  - waitUntil: { command: \"false\", timeout: 30m }\n")[0];
        assert_eq!(
            desktop.play_while(step, &mut alive_for(3)),
            Err(Error::Interrupted)
        );
        assert!(desktop.into_platform().into_runner().calls.len() <= 3);
    }

    #[test]
    fn sleeps_end_when_the_recording_stops() {
        let mut desktop = Desktop::new(Helper::new(Fake::default()));
        let step = &steps("  - sleep: 5m\n")[0];
        assert_eq!(
            desktop.play_while(step, &mut alive_for(3)),
            Err(Error::Interrupted)
        );
        assert!(desktop.into_platform().into_runner().slept <= Duration::from_secs(3));
    }

    #[test]
    fn wait_until_times_out() {
        let mut fake = Fake::default();
        for _ in 0..1000 {
            fake = fake.exit("sh -c", 1);
        }
        let err = play(fake, "  - waitUntil: { command: \"false\", timeout: 2s }\n").err();
        assert_eq!(err, Some(Error::ConditionNotMet("false".into())));
    }

    #[test]
    fn warp_markers_touch_nothing() {
        let fake = play(
            Fake::default(),
            "  - warp:\n      label: \"x\"\n      length: 1s\n      steps: []\n",
        )
        .unwrap();
        assert_eq!(fake.calls, Vec::<String>::new());
        assert_eq!(fake.slept, Duration::ZERO);
    }

    #[test]
    fn garbled_pointer_location_is_an_error() {
        for output in ["huh\n", "1\t2\t3\n", "-5\t7\n", "+5\t7\n", "5,7\n"] {
            let fake = Fake::default().reply(POINTER, output);
            let err = play(fake, "  - moveTo: { point: { x: 1, y: 1 } }\n").err();
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
}
