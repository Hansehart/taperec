//! Tapes: YAML documents of steps, parsed into what the desktop plays.
//!
//! The JSON Schema from [`schema`] lets editors complete and check tapes while they are written.
//!
//! ```yaml
//! output: open_terminal.mp4
//! steps:
//!   - set: { typingSpeed: 60ms, moveDuration: 700ms }
//!   - click: { dock: Terminal }
//!   - waitWindow: { title: Terminal }
//!   - focus: Terminal
//!   - paste: git clone https://github.com/Hansehart/o3s.git
//!   - key: primary+shift+p
//!   - warp:
//!       label: Installing
//!       length: 3s
//!       steps:
//!         - waitUntil: { command: command -v code, timeout: 5m }
//!   - click: { button: Open, timeout: 2m }
//! ```

use std::fmt;
use std::time::Duration;

use crate::units::{self, Finer};

mod document;

pub use document::schema;

/// A position on screen, in pixels from the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// Pixels from the left edge.
    pub x: u32,
    /// Pixels from the top edge.
    pub y: u32,
}

/// An item's exact name in the macOS Dock, e.g. `Terminal` or `System Settings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockItem(String);

impl DockItem {
    /// The Dock name as written in the tape.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// A modifier held during a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// The platform's main shortcut modifier: ⌘ on macOS.
    Primary,
    /// The ⌘ key.
    Cmd,
    /// The ⌃ key.
    Ctrl,
    /// The ⌥ key.
    Alt,
    /// The ⇧ key.
    Shift,
    /// The fn key.
    Fn,
}

impl Modifier {
    /// The name tapes write it with.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Cmd => "cmd",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Fn => "fn",
        }
    }
}

/// The key a chord presses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A named key, e.g. `return`.
    Named(&'static str),
    /// A printable ASCII character.
    Char(char),
}

/// Modifiers plus one key, e.g. `cmd+shift+p` or `return`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    modifiers: Vec<Modifier>,
    key: Key,
}

impl KeyChord {
    /// The modifiers, in the order written.
    #[must_use]
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    /// The key pressed while they are held.
    #[must_use]
    pub fn key(&self) -> &Key {
        &self.key
    }
}

/// `modifier+...+key`, the way tapes write it.
impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(f, "{}+", modifier.name())?;
        }
        match self.key {
            Key::Named(name) => f.write_str(name),
            Key::Char(c) => write!(f, "{c}"),
        }
    }
}

/// A button's exact label on screen, e.g. `Open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlLabel(String);

impl ControlLabel {
    /// The label as written in the tape.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An app's name as macOS knows it, e.g. `Terminal` or `Visual Studio Code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppName(String);

impl AppName {
    /// The name as written in the tape.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Text a visible window's title contains, matched case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowName(String);

impl WindowName {
    /// The text to look for.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A non-empty `sh -c` command polled by `WaitUntil`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand(String);

impl ShellCommand {
    /// The command line.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The non-empty caption shown while a warp plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarpLabel(String);

impl WarpLabel {
    /// The caption.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The default timeout of `WaitWindow` and `WaitUntil`.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(15);

/// The plain `.mp4` file name the recording is saved as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFile(String);

impl OutputFile {
    /// The file name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where the pointer should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An absolute screen position.
    Point(Point),
    /// The centre of a Dock icon.
    Dock(DockItem),
    /// The centre of a button found by its label, once it appears.
    Control {
        /// The button's label.
        label: ControlLabel,
        /// The longest wait for it to appear.
        timeout: Duration,
    },
}

/// One action of a tutorial, run in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Pause between keystrokes for following `Type` steps.
    TypingSpeed(Duration),
    /// Time following pointer moves take.
    MoveDuration(Duration),
    /// Wait for a while.
    Sleep(Duration),
    /// Glide the pointer to a target.
    MoveTo(Target),
    /// Glide the pointer to a target and left-click.
    Click(Target),
    /// Wait until a visible window's title contains the text.
    WaitWindow {
        /// Text the title contains.
        name: WindowName,
        /// The longest wait.
        timeout: Duration,
    },
    /// Bring an app to the front and wait until it is.
    Focus {
        /// The app.
        app: AppName,
        /// The longest wait.
        timeout: Duration,
    },
    /// Wait until a shell command succeeds.
    WaitUntil {
        /// Command polled with `sh -c`.
        command: ShellCommand,
        /// The longest wait.
        timeout: Duration,
    },
    /// Start of a stretch compressed into `length` in the final video.
    Warp {
        /// Caption shown while it plays.
        label: WarpLabel,
        /// Length in the final video.
        length: Duration,
    },
    /// End of the current warp.
    EndWarp,
    /// Type text keystroke by keystroke.
    Type(String),
    /// Put text on the clipboard and paste it with a single keystroke.
    Paste(String),
    /// Press a key chord.
    Key(KeyChord),
}

/// A parsed tutorial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tape {
    /// Where the recording goes.
    pub output: OutputFile,
    /// What happens on screen.
    pub steps: Vec<Step>,
}

/// Why a value in a tape is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ValueError {
    /// A malformed duration.
    Duration(String),
    /// A malformed key chord.
    Key(String),
    /// A malformed output file name.
    Output(String),
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duration(value) => write!(
                f,
                "expected a duration like `500ms`, `1.5s` or `2m`, got `{value}`"
            ),
            Self::Key(value) => write!(
                f,
                "expected keys like `return` or `primary+shift+p`, got `{value}`"
            ),
            Self::Output(value) => write!(
                f,
                "expected a plain file name ending in `.mp4`, got `{value}`"
            ),
        }
    }
}

/// Where in a tape a problem lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// A 1-based position in the YAML text.
    Position {
        /// The line.
        line: u64,
        /// The column.
        column: u64,
    },
    /// A step by its path, e.g. `steps[2].warp.steps[0]`.
    Step(String),
    /// The tape as a whole.
    Document,
}

/// Why a tape is invalid, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Where the problem lies.
    pub at: Where,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.at {
            Where::Position { line, column } => {
                write!(f, "line {line}, column {column}: {}", self.message)
            }
            Where::Step(path) => write!(f, "{path}: {}", self.message),
            Where::Document => self.message.fmt(f),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses a YAML tape.
///
/// # Errors
///
/// The first problem found, at its line and column or step path.
pub fn parse(source: &str) -> Result<Tape, ParseError> {
    let document: document::Document =
        serde_saphyr::from_str(source).map_err(|e| yaml_error(&e))?;
    document.lower().map_err(|violation| ParseError {
        at: Where::Step(violation.path),
        message: violation.message.to_owned(),
    })
}

/// A YAML or shape error, with its position when the parser knows it.
fn yaml_error(error: &serde_saphyr::Error) -> ParseError {
    let options = serde_saphyr::render_options! { snippets: serde_saphyr::SnippetMode::Off };
    let mut message = error.render_with_options(options);
    let at = error
        .location()
        .map_or(Where::Document, |location| Where::Position {
            line: location.line(),
            column: location.column(),
        });
    // The position is reported in `at`, so drop the copy the parser appends.
    if let Where::Position { line, column } = at {
        let suffix = format!(" at line {line}, column {column}");
        if let Some(stripped) = message.strip_suffix(&suffix) {
            message = stripped.to_owned();
        }
    }
    ParseError { at, message }
}

/// Parses `<int>ms`, `<decimal>s` or `<decimal>m` exactly.
fn duration(value: &str) -> Result<Duration, ValueError> {
    let bad = || ValueError::Duration(value.into());
    if let Some(millis) = value.strip_suffix("ms") {
        return units::natural(millis)
            .map(Duration::from_millis)
            .ok_or_else(bad);
    }
    if let Some(minutes) = value.strip_suffix('m') {
        return units::seconds(minutes, Finer::Reject)
            .and_then(|d| d.checked_mul(60))
            .ok_or_else(bad);
    }
    value
        .strip_suffix('s')
        .and_then(|seconds| units::seconds(seconds, Finer::Reject))
        .ok_or_else(bad)
}

/// Named keys tapes may press, cliclick's `kp:` names (`KeyPressAction.m`).
const NAMED_KEYS: [&str; 59] = [
    "return",
    "enter",
    "esc",
    "tab",
    "space",
    "delete",
    "fwd-delete",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "f13",
    "f14",
    "f15",
    "f16",
    "arrow-up",
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "home",
    "end",
    "page-up",
    "page-down",
    "num-0",
    "num-1",
    "num-2",
    "num-3",
    "num-4",
    "num-5",
    "num-6",
    "num-7",
    "num-8",
    "num-9",
    "num-clear",
    "num-equals",
    "num-divide",
    "num-multiply",
    "num-minus",
    "num-plus",
    "num-enter",
    "mute",
    "volume-up",
    "volume-down",
    "brightness-up",
    "brightness-down",
    "play-pause",
    "play-previous",
    "play-next",
    "keys-light-toggle",
    "keys-light-up",
    "keys-light-down",
];

/// Parses `[modifier+]...key`: distinct modifiers, then a named key or one printable character.
fn key_chord(value: &str) -> Result<KeyChord, ValueError> {
    let bad = || ValueError::Key(value.into());
    let mut parts: Vec<&str> = value.split('+').collect();
    let last = parts.pop().ok_or_else(bad)?;
    let mut modifiers = Vec::with_capacity(parts.len());
    for part in parts {
        let modifier = match part {
            "primary" => Modifier::Primary,
            "cmd" => Modifier::Cmd,
            "ctrl" => Modifier::Ctrl,
            "alt" => Modifier::Alt,
            "shift" => Modifier::Shift,
            "fn" => Modifier::Fn,
            _ => return Err(bad()),
        };
        if modifiers.contains(&modifier) {
            return Err(bad());
        }
        modifiers.push(modifier);
    }
    let key = if let Some(name) = NAMED_KEYS.iter().find(|name| **name == last) {
        Key::Named(name)
    } else {
        let mut chars = last.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_graphic() && c != '+' => Key::Char(c),
            _ => return Err(bad()),
        }
    };
    Ok(KeyChord { modifiers, key })
}

fn output_file(value: &str) -> Result<OutputFile, ValueError> {
    let plain = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.');
    let valid = value
        .strip_suffix(".mp4")
        .is_some_and(|stem| !stem.is_empty())
        && !value.starts_with('.')
        && value.bytes().all(plain);
    if valid {
        Ok(OutputFile(value.into()))
    } else {
        Err(ValueError::Output(value.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Wraps YAML step items into a tape.
    fn tape(items: &str) -> String {
        format!("output: t.mp4\nsteps:\n{items}")
    }

    fn steps(items: &str) -> Vec<Step> {
        parse(&tape(items)).unwrap().steps
    }

    fn error(items: &str) -> ParseError {
        parse(&tape(items)).unwrap_err()
    }

    fn at_step(path: &str) -> Where {
        Where::Step(path.into())
    }

    #[test]
    fn parses_every_step() {
        let source = r"
output: demo.mp4
steps:
  - set: { typingSpeed: 60ms, moveDuration: 0.7s }
  - sleep: 1.5s
  - moveTo: { point: { x: 960, y: 540 } }
  - click: { dock: Terminal }
  - waitWindow: { title: Terminal }
  - focus: Terminal
  - type: echo hi
  - paste: git clone https://github.com/Hansehart/o3s.git
  - key: cmd+shift+p
  - waitUntil: { command: command -v code, timeout: 5m }
  - click: { button: Open, timeout: 2m }
";
        assert_eq!(
            parse(source).unwrap(),
            Tape {
                output: OutputFile("demo.mp4".into()),
                steps: vec![
                    Step::TypingSpeed(ms(60)),
                    Step::MoveDuration(ms(700)),
                    Step::Sleep(ms(1500)),
                    Step::MoveTo(Target::Point(Point { x: 960, y: 540 })),
                    Step::Click(Target::Dock(DockItem("Terminal".into()))),
                    Step::WaitWindow {
                        name: WindowName("Terminal".into()),
                        timeout: DEFAULT_WAIT,
                    },
                    Step::Focus {
                        app: AppName("Terminal".into()),
                        timeout: DEFAULT_WAIT,
                    },
                    Step::Type("echo hi".into()),
                    Step::Paste("git clone https://github.com/Hansehart/o3s.git".into()),
                    Step::Key(KeyChord {
                        modifiers: vec![Modifier::Cmd, Modifier::Shift],
                        key: Key::Char('p'),
                    }),
                    Step::WaitUntil {
                        command: ShellCommand("command -v code".into()),
                        timeout: ms(300_000),
                    },
                    Step::Click(Target::Control {
                        label: ControlLabel("Open".into()),
                        timeout: ms(120_000),
                    }),
                ],
            }
        );
    }

    #[test]
    fn yaml_quoting_and_comments_carry_over() {
        assert_eq!(
            steps("  # a comment\n  - type: \"say \\\"hi\\\" \\\\ bye\\n\"\n  - type: 'it''s'\n"),
            vec![
                Step::Type("say \"hi\" \\ bye\n".into()),
                Step::Type("it's".into())
            ]
        );
    }

    #[test]
    fn unknown_steps_and_fields_point_at_their_line() {
        let err = error("  - sleep: 1s\n  - jump: 3\n");
        assert!(matches!(err.at, Where::Position { line: 4, .. }), "{err}");
        assert!(err.message.contains("jump"), "{err}");
        let err = error("  - waitWindow: { title: x, wait: 2s }\n");
        assert!(matches!(err.at, Where::Position { line: 3, .. }), "{err}");
        assert!(err.message.contains("wait"), "{err}");
    }

    #[test]
    fn rejects_bad_durations() {
        for bad in ["fast", "3", "-1s", "1.5ms", "s", "1h", ""] {
            let err = error(&format!("  - sleep: \"{bad}\"\n"));
            assert!(
                matches!(err.at, Where::Position { line: 3, .. }),
                "{bad}: {err}"
            );
            assert!(
                err.message
                    .contains(&ValueError::Duration(bad.into()).to_string()),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn durations_take_minutes() {
        assert_eq!(
            steps("  - sleep: 2m\n  - sleep: 1.5m\n"),
            vec![Step::Sleep(ms(120_000)), Step::Sleep(ms(90_000))]
        );
    }

    #[test]
    fn rejects_bad_keys() {
        for bad in [
            "cmd+",
            "+p",
            "cmd++p",
            "Return",
            "ctrl+shift",
            "cmd+cmd+a",
            "hyper+a",
            "ab",
            "cmd+é",
        ] {
            let err = error(&format!("  - key: \"{bad}\"\n"));
            assert!(
                err.message
                    .contains(&ValueError::Key(bad.into()).to_string()),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn rejects_bad_outputs() {
        for bad in ["demo.mov", "dir/demo.mp4", ".mp4", "../demo.mp4"] {
            let err = parse(&format!("output: \"{bad}\"\nsteps: []\n")).unwrap_err();
            assert!(
                err.message
                    .contains(&ValueError::Output(bad.into()).to_string()),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn requires_output_and_steps() {
        let err = parse("steps: []\n").unwrap_err();
        assert!(err.message.contains("output"), "{err}");
        let err = parse("output: t.mp4\n").unwrap_err();
        assert!(err.message.contains("steps"), "{err}");
    }

    #[test]
    fn names_must_not_be_empty() {
        for items in [
            "  - focus: \"\"\n",
            "  - waitWindow: { title: \" \" }\n",
            "  - waitUntil: { command: \"\" }\n",
            "  - click: { button: \"\" }\n",
        ] {
            let err = error(items);
            assert!(err.message.contains("must not be empty"), "{items}: {err}");
        }
    }

    #[test]
    fn targets_take_exactly_one_kind() {
        assert_eq!(error("  - click: {}\n").at, at_step("steps[0]"));
        assert_eq!(
            error("  - click: { dock: Finder, button: Open }\n"),
            ParseError {
                at: at_step("steps[0]"),
                message: "a target takes exactly one of `point`, `dock` or `button`".into(),
            }
        );
        assert_eq!(
            error("  - sleep: 1s\n  - moveTo: { dock: Finder, timeout: 2s }\n"),
            ParseError {
                at: at_step("steps[1]"),
                message: "`timeout` only applies to a `button` target".into(),
            }
        );
    }

    #[test]
    fn buttons_wait_fifteen_seconds_unless_told() {
        assert_eq!(
            steps("  - click: { button: Reopen in Container }\n"),
            vec![Step::Click(Target::Control {
                label: ControlLabel("Reopen in Container".into()),
                timeout: DEFAULT_WAIT,
            })]
        );
    }

    #[test]
    fn warps_nest_their_steps() {
        let items = "  - warp:\n      label: Build\n      length: 5s\n      steps:\n        - sleep: 1s\n  - sleep: 2s\n";
        assert_eq!(
            steps(items),
            vec![
                Step::Warp {
                    label: WarpLabel("Build".into()),
                    length: ms(5000),
                },
                Step::Sleep(ms(1000)),
                Step::EndWarp,
                Step::Sleep(ms(2000)),
            ]
        );
    }

    #[test]
    fn rejects_bad_warps() {
        let nested = "  - warp:\n      label: a\n      length: 1s\n      steps:\n        - warp: { label: b, length: 1s, steps: [] }\n";
        assert_eq!(
            error(nested),
            ParseError {
                at: at_step("steps[0].warp.steps[0]"),
                message: "a warp cannot sit inside another warp".into(),
            }
        );
        assert_eq!(
            error("  - warp: { label: a, length: 0s, steps: [] }\n"),
            ParseError {
                at: at_step("steps[0]"),
                message: "a warp must last longer than zero".into(),
            }
        );
    }

    #[test]
    fn set_needs_a_speed() {
        assert_eq!(
            steps("  - set: { moveDuration: 1s }\n"),
            vec![Step::MoveDuration(ms(1000))]
        );
        assert_eq!(error("  - set: {}\n").at, at_step("steps[0]"));
    }

    #[test]
    fn errors_read_with_their_place() {
        let positioned = ParseError {
            at: Where::Position { line: 3, column: 9 },
            message: "unknown field".into(),
        };
        assert_eq!(positioned.to_string(), "line 3, column 9: unknown field");
        assert_eq!(
            error("  - set: {}\n").to_string(),
            "steps[0]: `set` needs `typingSpeed` or `moveDuration`"
        );
    }

    #[test]
    fn key_chords_parse_modifiers_and_key() {
        let chord =
            |modifiers: Vec<Modifier>, key: Key| vec![Step::Key(KeyChord { modifiers, key })];
        assert_eq!(
            steps("  - key: return\n"),
            chord(vec![], Key::Named("return"))
        );
        assert_eq!(
            steps("  - key: cmd+space\n"),
            chord(vec![Modifier::Cmd], Key::Named("space"))
        );
        assert_eq!(steps("  - key: f12\n"), chord(vec![], Key::Named("f12")));
        assert_eq!(
            steps("  - key: ctrl+alt+fn+arrow-left\n"),
            chord(
                vec![Modifier::Ctrl, Modifier::Alt, Modifier::Fn],
                Key::Named("arrow-left")
            )
        );
        assert_eq!(steps("  - key: a\n"), chord(vec![], Key::Char('a')));
        assert_eq!(
            steps("  - key: \"shift+1\"\n"),
            chord(vec![Modifier::Shift], Key::Char('1'))
        );
    }

    #[test]
    fn primary_stays_primary_until_the_platform() {
        assert_eq!(
            steps("  - key: primary+shift+p\n"),
            vec![Step::Key(KeyChord {
                modifiers: vec![Modifier::Primary, Modifier::Shift],
                key: Key::Char('p'),
            })]
        );
        assert_eq!(Modifier::Primary.name(), "primary");
    }

    #[test]
    fn key_chords_expose_modifiers_and_key() {
        let Step::Key(chord) = &steps("  - key: ctrl+alt+fn+arrow-left\n")[0] else {
            panic!("not a key step");
        };
        assert_eq!(
            chord.modifiers(),
            [Modifier::Ctrl, Modifier::Alt, Modifier::Fn]
        );
        assert_eq!(chord.key(), &Key::Named("arrow-left"));
        assert_eq!(chord.to_string(), "ctrl+alt+fn+arrow-left");
    }

    #[test]
    fn the_swift_helper_knows_the_same_named_keys() {
        let listed = NAMED_KEYS.join("\n") + "\n";
        assert_eq!(
            include_str!("../tools/desktop/Tests/DesktopKitTests/named-keys.txt"),
            listed,
            "regenerate tools/desktop/Tests/DesktopKitTests/named-keys.txt from NAMED_KEYS"
        );
    }

    #[test]
    fn committed_schema_is_current() {
        assert_eq!(
            include_str!("../schema/taperec.schema.json"),
            schema(),
            "regenerate with ./test.sh run -q -- schema > schema/taperec.schema.json"
        );
    }

    #[test]
    fn schema_marks_platform_extras_and_target_kinds() {
        let schema = schema();
        assert!(schema.contains("\"x-platforms\""), "{schema}");
        assert!(schema.contains("\"oneOf\""), "{schema}");
        assert!(
            schema.contains("\"primary+shift+p\"") || schema.contains("primary"),
            "{schema}"
        );
    }
}
