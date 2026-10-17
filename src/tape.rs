//! Parser for `.tape` tutorial scripts.
//!
//! One command per line; `#` starts a comment. Exactly one `Output` is required.
//!
//! ```text
//! Output open_terminal.mp4
//! Set TypingSpeed 60ms
//! Set MoveDuration 700ms
//! Sleep 1.5s
//! MoveTo 960,540
//! Click dock:Terminal
//! WaitWindow "Terminal"
//! Focus "Terminal"
//! Type "echo \"hi\""
//! Paste "git clone https://github.com/Hansehart/o3s.git"
//! Key cmd+shift+p
//! Warp "Installing" 3s
//! WaitUntil "command -v code" 5m
//! EndWarp
//! Click button:"Open" 2m
//! ```

use std::fmt;
use std::time::Duration;

use crate::units::{self, Finer};

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

/// A modifier held during a chord, by its cliclick `kd:`/`ku:` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
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
    /// The name cliclick's `kd:` and `ku:` take.
    #[must_use]
    pub fn cliclick_name(self) -> &'static str {
        match self {
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
    /// A named key cliclick's `kp:` knows, e.g. `return`.
    Named(&'static str),
    /// A printable ASCII character, typed with `t:`.
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

/// Why a tape failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// An unknown command word.
    UnknownCommand(String),
    /// An unknown `Set` setting.
    UnknownSetting(String),
    /// A command missing its argument.
    MissingArgument,
    /// Extra input after a command's arguments.
    TrailingInput(String),
    /// A malformed duration; durations look like `500ms`, `1.5s` or `2m`.
    BadDuration(String),
    /// A malformed target; targets look like `x,y`, `dock:<name>` or `button:"<label>"`.
    BadTarget(String),
    /// A malformed key chord; chords look like `return` or `cmd+shift+p`.
    BadKey(String),
    /// A malformed output; outputs are plain `.mp4` file names.
    BadOutput(String),
    /// A string argument missing its opening `"`.
    ExpectedQuote,
    /// A quoted string missing its closing `"`.
    UnterminatedString,
    /// A quoted string uses an unknown escape.
    BadEscape(char),
    /// An empty `WaitWindow` text.
    EmptyWindowName,
    /// An empty `button:` label or `Focus` app name.
    EmptyName,
    /// An empty `WaitUntil` command.
    EmptyCommand,
    /// An empty `Warp` label.
    EmptyWarpLabel,
    /// A zero-length `Warp`.
    ZeroWarpLength,
    /// A `Warp` inside another warp.
    NestedWarp,
    /// An `EndWarp` outside any warp.
    UnmatchedEndWarp,
    /// A `Warp` still open at the end, reported at its line.
    UnclosedWarp,
    /// A second `Output` line.
    DuplicateOutput,
    /// A tape missing its `Output` line, reported at line 0.
    MissingOutput,
}

/// A parse failure and the 1-based line it occurred on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number, or 0 for whole-file errors.
    pub line: usize,
    /// What went wrong.
    pub kind: ParseErrorKind,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(command) => write!(f, "unknown command `{command}`"),
            Self::UnknownSetting(setting) => write!(
                f,
                "unknown setting `{setting}`; expected `TypingSpeed` or `MoveDuration`"
            ),
            Self::MissingArgument => write!(f, "missing argument"),
            Self::TrailingInput(rest) => write!(f, "unexpected `{rest}` at the end of the line"),
            Self::BadDuration(value) => write!(
                f,
                "expected a duration like `500ms`, `1.5s` or `2m`, got `{value}`"
            ),
            Self::BadTarget(value) => {
                write!(
                    f,
                    "expected `x,y`, `dock:<name>`, `dock:\"<name>\"` or `button:\"<label>\"`, got `{value}`"
                )
            }
            Self::BadKey(value) => write!(
                f,
                "expected keys like `return` or `cmd+shift+p`, got `{value}`"
            ),
            Self::BadOutput(value) => write!(
                f,
                "expected a plain file name ending in `.mp4`, got `{value}`"
            ),
            Self::ExpectedQuote => write!(f, "expected a \"quoted\" string"),
            Self::UnterminatedString => write!(f, "unterminated string"),
            Self::BadEscape(c) => write!(
                f,
                "unknown escape `\\{c}`; use `\\\"`, `\\\\`, `\\n` or `\\t`"
            ),
            Self::EmptyWindowName => write!(f, "the window pattern is empty"),
            Self::EmptyName => write!(f, "the name is empty"),
            Self::EmptyCommand => write!(f, "the command is empty"),
            Self::EmptyWarpLabel => write!(f, "the warp label is empty"),
            Self::ZeroWarpLength => write!(f, "a warp must last longer than zero"),
            Self::NestedWarp => write!(
                f,
                "`Warp` inside another warp; close it with `EndWarp` first"
            ),
            Self::UnmatchedEndWarp => write!(f, "`EndWarp` without a `Warp`"),
            Self::UnclosedWarp => write!(f, "this `Warp` is never closed with `EndWarp`"),
            Self::DuplicateOutput => write!(f, "a second `Output`; a tape has exactly one"),
            Self::MissingOutput => write!(f, "no `Output` line"),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Line 0 marks whole-tape errors.
        match self.line {
            0 => self.kind.fmt(f),
            line => write!(f, "line {line}: {}", self.kind),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses a whole tape.
///
/// # Errors
///
/// Returns the first problem found, with its line number.
pub fn parse(source: &str) -> Result<Tape, ParseError> {
    let mut output = None;
    let mut steps = Vec::new();
    // Line of the warp currently open, if any.
    let mut open_warp = None;
    for (index, raw) in source.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let at = |kind| ParseError { line, kind };
        match parse_line(text).map_err(at)? {
            Line::Output(file) => {
                if output.replace(file).is_some() {
                    return Err(at(ParseErrorKind::DuplicateOutput));
                }
            }
            Line::Step(step) => {
                match step {
                    Step::Warp { .. } if open_warp.is_some() => {
                        return Err(at(ParseErrorKind::NestedWarp));
                    }
                    Step::Warp { .. } => open_warp = Some(line),
                    Step::EndWarp if open_warp.take().is_none() => {
                        return Err(at(ParseErrorKind::UnmatchedEndWarp));
                    }
                    _ => {}
                }
                steps.push(step);
            }
        }
    }
    if let Some(line) = open_warp {
        return Err(ParseError {
            line,
            kind: ParseErrorKind::UnclosedWarp,
        });
    }
    let output = output.ok_or(ParseError {
        line: 0,
        kind: ParseErrorKind::MissingOutput,
    })?;
    Ok(Tape { output, steps })
}

enum Line {
    Output(OutputFile),
    Step(Step),
}

fn parse_line(text: &str) -> Result<Line, ParseErrorKind> {
    let (command, rest) = split_word(text);
    let step = match command {
        "Output" => {
            let (name, rest) = word(rest)?;
            end(rest)?;
            return output_file(name).map(Line::Output);
        }
        "Set" => {
            let (setting, rest) = word(rest)?;
            let (value, rest) = word(rest)?;
            end(rest)?;
            match setting {
                "TypingSpeed" => Step::TypingSpeed(duration(value)?),
                "MoveDuration" => Step::MoveDuration(duration(value)?),
                other => return Err(ParseErrorKind::UnknownSetting(other.into())),
            }
        }
        "Sleep" => Step::Sleep(single(rest, duration)?),
        "MoveTo" => Step::MoveTo(target(rest)?),
        "Click" => Step::Click(target(rest)?),
        "Key" => Step::Key(single(rest, key_chord)?),
        "Type" => {
            let (text, rest) = quoted(rest)?;
            end(rest)?;
            Step::Type(text)
        }
        "Paste" => {
            let (text, rest) = quoted(rest)?;
            end(rest)?;
            Step::Paste(text)
        }
        "WaitWindow" => {
            let (name, rest) = quoted(rest)?;
            let timeout = optional_timeout(rest)?;
            if name.is_empty() {
                return Err(ParseErrorKind::EmptyWindowName);
            }
            Step::WaitWindow {
                name: WindowName(name),
                timeout,
            }
        }
        "Focus" => {
            let (app, rest) = quoted(rest)?;
            let timeout = optional_timeout(rest)?;
            if app.is_empty() {
                return Err(ParseErrorKind::EmptyName);
            }
            Step::Focus {
                app: AppName(app),
                timeout,
            }
        }
        "WaitUntil" => {
            let (command, rest) = quoted(rest)?;
            let timeout = optional_timeout(rest)?;
            if command.is_empty() {
                return Err(ParseErrorKind::EmptyCommand);
            }
            Step::WaitUntil {
                command: ShellCommand(command),
                timeout,
            }
        }
        "Warp" => {
            let (label, rest) = quoted(rest)?;
            if label.is_empty() {
                return Err(ParseErrorKind::EmptyWarpLabel);
            }
            let length = single(rest, duration)?;
            if length.is_zero() {
                return Err(ParseErrorKind::ZeroWarpLength);
            }
            Step::Warp {
                label: WarpLabel(label),
                length,
            }
        }
        "EndWarp" => {
            end(rest)?;
            Step::EndWarp
        }
        other => return Err(ParseErrorKind::UnknownCommand(other.into())),
    };
    Ok(Line::Step(step))
}

/// Splits off the first whitespace-delimited word; the rest is left-trimmed.
fn split_word(text: &str) -> (&str, &str) {
    match text.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim_start()),
        None => (text, ""),
    }
}

fn word(rest: &str) -> Result<(&str, &str), ParseErrorKind> {
    if rest.is_empty() {
        return Err(ParseErrorKind::MissingArgument);
    }
    Ok(split_word(rest))
}

fn end(rest: &str) -> Result<(), ParseErrorKind> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(ParseErrorKind::TrailingInput(rest.into()))
    }
}

/// An optional trailing duration, defaulting to [`DEFAULT_WAIT`].
fn optional_timeout(rest: &str) -> Result<Duration, ParseErrorKind> {
    if rest.is_empty() {
        Ok(DEFAULT_WAIT)
    } else {
        single(rest, duration)
    }
}

/// Parses exactly one word with `convert`.
fn single<T>(
    rest: &str,
    convert: fn(&str) -> Result<T, ParseErrorKind>,
) -> Result<T, ParseErrorKind> {
    let (value, rest) = word(rest)?;
    end(rest)?;
    convert(value)
}

/// Parses a `"`-quoted string with `\"`, `\\`, `\n` and `\t` escapes.
fn quoted(rest: &str) -> Result<(String, &str), ParseErrorKind> {
    let mut chars = rest.char_indices();
    match chars.next() {
        None => return Err(ParseErrorKind::MissingArgument),
        Some((_, '"')) => {}
        Some(_) => return Err(ParseErrorKind::ExpectedQuote),
    }
    let mut text = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => {
                let after = rest.get(index + 1..).unwrap_or_default();
                return Ok((text, after.trim_start()));
            }
            '\\' => match chars.next() {
                Some((_, '"')) => text.push('"'),
                Some((_, '\\')) => text.push('\\'),
                Some((_, 'n')) => text.push('\n'),
                Some((_, 't')) => text.push('\t'),
                Some((_, other)) => return Err(ParseErrorKind::BadEscape(other)),
                None => return Err(ParseErrorKind::UnterminatedString),
            },
            _ => text.push(c),
        }
    }
    Err(ParseErrorKind::UnterminatedString)
}

/// Parses `<int>ms`, `<decimal>s` or `<decimal>m` exactly.
fn duration(value: &str) -> Result<Duration, ParseErrorKind> {
    let bad = || ParseErrorKind::BadDuration(value.into());
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

/// Parses `x,y`, `dock:<name>`, `dock:"<name with spaces>"` or `button:"<label>" [timeout]` as the whole rest of the line.
fn target(rest: &str) -> Result<Target, ParseErrorKind> {
    if let Some(quoted_label) = rest.strip_prefix("button:") {
        let (label, rest) = quoted(quoted_label)?;
        let timeout = optional_timeout(rest)?;
        if label.is_empty() {
            return Err(ParseErrorKind::EmptyName);
        }
        return Ok(Target::Control {
            label: ControlLabel(label),
            timeout,
        });
    }
    if let Some(quoted_name) = rest.strip_prefix("dock:").filter(|r| r.starts_with('"')) {
        let (name, rest) = quoted(quoted_name).map_err(|e| match e {
            ParseErrorKind::MissingArgument => ParseErrorKind::BadTarget(quoted_name.into()),
            other => other,
        })?;
        end(rest)?;
        return dock_item(name).map(Target::Dock);
    }
    single(rest, |value| {
        let bad = || ParseErrorKind::BadTarget(value.into());
        if let Some(name) = value.strip_prefix("dock:") {
            return dock_item(name.into()).map(Target::Dock).map_err(|_| bad());
        }
        let (x, y) = value.split_once(',').ok_or_else(bad)?;
        let coordinate = |s: &str| units::natural(s).ok_or_else(bad);
        Ok(Target::Point(Point {
            x: coordinate(x)?,
            y: coordinate(y)?,
        }))
    })
}

/// A Dock name made of characters that stay inside an `AppleScript` string literal.
fn dock_item(name: String) -> Result<DockItem, ParseErrorKind> {
    if name.is_empty()
        || name
            .chars()
            .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        Err(ParseErrorKind::BadTarget(name))
    } else {
        Ok(DockItem(name))
    }
}

/// Named keys cliclick's `kp:` accepts (`KeyPressAction.m`).
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
fn key_chord(value: &str) -> Result<KeyChord, ParseErrorKind> {
    let bad = || ParseErrorKind::BadKey(value.into());
    let mut parts: Vec<&str> = value.split('+').collect();
    let last = parts.pop().ok_or_else(bad)?;
    let mut modifiers = Vec::with_capacity(parts.len());
    for part in parts {
        let modifier = match part {
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

fn output_file(value: &str) -> Result<OutputFile, ParseErrorKind> {
    let plain = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.');
    let valid = value
        .strip_suffix(".mp4")
        .is_some_and(|stem| !stem.is_empty())
        && !value.starts_with('.')
        && value.bytes().all(plain);
    if valid {
        Ok(OutputFile(value.into()))
    } else {
        Err(ParseErrorKind::BadOutput(value.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn kind(source: &str) -> ParseErrorKind {
        parse(source).unwrap_err().kind
    }

    fn steps(body: &str) -> Vec<Step> {
        parse(&format!("Output t.mp4\n{body}")).unwrap().steps
    }

    #[test]
    fn parses_every_command() {
        let tape = r#"
            Output demo.mp4
            Set TypingSpeed 60ms
            Set MoveDuration 0.7s
            Sleep 1.5s
            MoveTo 960,540
            Click dock:Terminal
            WaitWindow "Terminal"
            Type "echo hi"
            Key cmd+shift+p
        "#;
        assert_eq!(
            parse(tape).unwrap(),
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
                    Step::Type("echo hi".into()),
                    Step::Key(KeyChord {
                        modifiers: vec![Modifier::Cmd, Modifier::Shift],
                        key: Key::Char('p'),
                    }),
                ],
            }
        );
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        assert_eq!(
            steps("# intro\n\n  # more\nSleep 1s\n"),
            vec![Step::Sleep(ms(1000))]
        );
    }

    #[test]
    fn unescapes_quoted_strings() {
        assert_eq!(
            steps(r#"Type "say \"hi\" \\ bye\n""#),
            vec![Step::Type("say \"hi\" \\ bye\n".into())]
        );
    }

    #[test]
    fn reports_line_of_unknown_command() {
        let err = parse("Output t.mp4\nJump 3\n").unwrap_err();
        assert_eq!(
            err,
            ParseError {
                line: 2,
                kind: ParseErrorKind::UnknownCommand("Jump".into())
            }
        );
    }

    #[test]
    fn rejects_unknown_setting() {
        assert_eq!(
            kind("Set Speed 1s"),
            ParseErrorKind::UnknownSetting("Speed".into())
        );
    }

    #[test]
    fn rejects_bad_durations() {
        for bad in ["fast", "3", "-1s", "1.5ms", "s"] {
            assert_eq!(
                kind(&format!("Sleep {bad}")),
                ParseErrorKind::BadDuration(bad.into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_bad_targets() {
        for bad in ["10", "dock:", "a,b", "-1,5", "+5,7", "1,2,3"] {
            assert_eq!(
                kind(&format!("Click {bad}")),
                ParseErrorKind::BadTarget(bad.into()),
                "{bad}"
            );
        }
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
            assert_eq!(
                kind(&format!("Key {bad}")),
                ParseErrorKind::BadKey(bad.into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_bad_outputs() {
        for bad in ["demo.mov", "dir/demo.mp4", ".mp4", "../demo.mp4"] {
            assert_eq!(
                kind(&format!("Output {bad}")),
                ParseErrorKind::BadOutput(bad.into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_unterminated_string() {
        assert_eq!(kind("Type \"oops"), ParseErrorKind::UnterminatedString);
    }

    #[test]
    fn rejects_unknown_escape() {
        assert_eq!(kind(r#"Type "a\qb""#), ParseErrorKind::BadEscape('q'));
    }

    #[test]
    fn rejects_unquoted_text() {
        assert_eq!(kind("Type hello"), ParseErrorKind::ExpectedQuote);
    }

    #[test]
    fn rejects_empty_window_name() {
        assert_eq!(kind(r#"WaitWindow """#), ParseErrorKind::EmptyWindowName);
    }

    #[test]
    fn rejects_missing_argument() {
        for bad in ["Key", "Output", "Sleep", "Set", "Set TypingSpeed", "Type"] {
            assert_eq!(kind(bad), ParseErrorKind::MissingArgument, "{bad}");
        }
    }

    #[test]
    fn rejects_trailing_input() {
        assert_eq!(
            kind("Sleep 1s 2s"),
            ParseErrorKind::TrailingInput("2s".into())
        );
        assert_eq!(
            kind(r#"Type "a" b"#),
            ParseErrorKind::TrailingInput("b".into())
        );
    }

    #[test]
    fn requires_exactly_one_output() {
        assert_eq!(
            parse("Sleep 1s").unwrap_err(),
            ParseError {
                line: 0,
                kind: ParseErrorKind::MissingOutput
            }
        );
        assert_eq!(
            parse("Output a.mp4\nOutput b.mp4").unwrap_err(),
            ParseError {
                line: 2,
                kind: ParseErrorKind::DuplicateOutput
            }
        );
    }

    #[test]
    fn durations_take_minutes() {
        assert_eq!(
            steps("Sleep 45m"),
            vec![Step::Sleep(Duration::from_mins(45))]
        );
        assert_eq!(
            steps("Sleep 1.5m"),
            vec![Step::Sleep(Duration::from_secs(90))]
        );
        for bad in ["m", "-1m", "1.5mm", "1.m5"] {
            assert_eq!(
                kind(&format!("Sleep {bad}")),
                ParseErrorKind::BadDuration(bad.into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn waits_take_optional_timeouts() {
        assert_eq!(
            steps("WaitWindow \"^o3s - .+ - Visual Studio Code$\" 60m\nWaitUntil \"command -v code\"\nWaitUntil \"true\" 5s"),
            vec![
                Step::WaitWindow {
                    name: WindowName("^o3s - .+ - Visual Studio Code$".into()),
                    timeout: Duration::from_hours(1),
                },
                Step::WaitUntil {
                    command: ShellCommand("command -v code".into()),
                    timeout: DEFAULT_WAIT,
                },
                Step::WaitUntil {
                    command: ShellCommand("true".into()),
                    timeout: Duration::from_secs(5),
                },
            ]
        );
        assert_eq!(kind("WaitUntil \"\""), ParseErrorKind::EmptyCommand);
        assert_eq!(
            kind("WaitUntil \"x\" soon"),
            ParseErrorKind::BadDuration("soon".into())
        );
        assert_eq!(
            kind("WaitWindow \"x\" 1s 2s"),
            ParseErrorKind::TrailingInput("2s".into())
        );
    }

    #[test]
    fn parses_warps() {
        assert_eq!(
            steps("Warp \"Building the cage\" 5s\nSleep 1s\nEndWarp"),
            vec![
                Step::Warp {
                    label: WarpLabel("Building the cage".into()),
                    length: Duration::from_secs(5),
                },
                Step::Sleep(Duration::from_secs(1)),
                Step::EndWarp,
            ]
        );
    }

    #[test]
    fn rejects_bad_warps() {
        assert_eq!(kind("Warp \"\" 3s"), ParseErrorKind::EmptyWarpLabel);
        assert_eq!(kind("Warp \"x\" 0s"), ParseErrorKind::ZeroWarpLength);
        assert_eq!(kind("Warp \"x\""), ParseErrorKind::MissingArgument);
        assert_eq!(
            kind("EndWarp now"),
            ParseErrorKind::TrailingInput("now".into())
        );
    }

    #[test]
    fn warps_must_balance() {
        let at = |source: &str| parse(&format!("Output t.mp4\n{source}")).unwrap_err();
        assert_eq!(
            at("Warp \"a\" 1s\nWarp \"b\" 1s\nEndWarp\nEndWarp"),
            ParseError {
                line: 3,
                kind: ParseErrorKind::NestedWarp
            }
        );
        assert_eq!(
            at("Sleep 1s\nEndWarp"),
            ParseError {
                line: 3,
                kind: ParseErrorKind::UnmatchedEndWarp
            }
        );
        assert_eq!(
            at("Warp \"a\" 1s\nSleep 1s"),
            ParseError {
                line: 2,
                kind: ParseErrorKind::UnclosedWarp
            }
        );
    }

    #[test]
    fn errors_read_as_plain_english() {
        let cases = [
            ("Output a.mp4\nJump 3", "line 2: unknown command `Jump`"),
            (
                "Output a.mp4\nSet Speed 1s",
                "line 2: unknown setting `Speed`; expected `TypingSpeed` or `MoveDuration`",
            ),
            ("Output a.mp4\nKey", "line 2: missing argument"),
            (
                "Output a.mp4\nSleep 1s 2s",
                "line 2: unexpected `2s` at the end of the line",
            ),
            (
                "Output a.mp4\nSleep fast",
                "line 2: expected a duration like `500ms`, `1.5s` or `2m`, got `fast`",
            ),
            (
                "Output a.mp4\nClick 10",
                "line 2: expected `x,y`, `dock:<name>`, `dock:\"<name>\"` or `button:\"<label>\"`, got `10`",
            ),
            (
                "Output a.mp4\nKey ctrl+",
                "line 2: expected keys like `return` or `cmd+shift+p`, got `ctrl+`",
            ),
            (
                "Output demo.mov",
                "line 1: expected a plain file name ending in `.mp4`, got `demo.mov`",
            ),
            (
                "Output a.mp4\nType hi",
                "line 2: expected a \"quoted\" string",
            ),
            ("Output a.mp4\nType \"hi", "line 2: unterminated string"),
            (
                "Output a.mp4\nType \"a\\qb\"",
                "line 2: unknown escape `\\q`; use `\\\"`, `\\\\`, `\\n` or `\\t`",
            ),
            (
                "Output a.mp4\nWaitWindow \"\"",
                "line 2: the window pattern is empty",
            ),
            (
                "Output a.mp4\nWaitUntil \"\"",
                "line 2: the command is empty",
            ),
            (
                "Output a.mp4\nWarp \"\" 1s\nEndWarp",
                "line 2: the warp label is empty",
            ),
            (
                "Output a.mp4\nWarp \"x\" 0s\nEndWarp",
                "line 2: a warp must last longer than zero",
            ),
            (
                "Output a.mp4\nWarp \"a\" 1s\nWarp \"b\" 1s",
                "line 3: `Warp` inside another warp; close it with `EndWarp` first",
            ),
            (
                "Output a.mp4\nEndWarp",
                "line 2: `EndWarp` without a `Warp`",
            ),
            (
                "Output a.mp4\nWarp \"a\" 1s",
                "line 2: this `Warp` is never closed with `EndWarp`",
            ),
            (
                "Output a.mp4\nOutput b.mp4",
                "line 2: a second `Output`; a tape has exactly one",
            ),
            ("Sleep 1s", "no `Output` line"),
        ];
        for (tape, message) in cases {
            assert_eq!(parse(tape).unwrap_err().to_string(), message, "{tape:?}");
        }
    }

    #[test]
    fn dock_names_may_be_quoted() {
        let dock = |name: &str| Step::Click(Target::Dock(DockItem(name.into())));
        assert_eq!(steps("Click dock:Terminal"), [dock("Terminal")]);
        assert_eq!(
            steps("Click dock:\"System Settings\""),
            [dock("System Settings")]
        );
        assert_eq!(
            steps("MoveTo dock:\"App Store\""),
            [Step::MoveTo(Target::Dock(DockItem("App Store".into())))]
        );
    }

    #[test]
    fn buttons_are_found_by_quoted_label_with_an_optional_wait() {
        let button = |label: &str, timeout| {
            Step::Click(Target::Control {
                label: ControlLabel(label.into()),
                timeout,
            })
        };
        assert_eq!(
            steps("Click button:\"Open\""),
            [button("Open", DEFAULT_WAIT)]
        );
        assert_eq!(
            steps("Click button:\"Reopen in Container\" 2m"),
            [button("Reopen in Container", Duration::from_secs(120))]
        );
        assert_eq!(kind("Click button:Open"), ParseErrorKind::ExpectedQuote);
        assert_eq!(kind("Click button:\"\""), ParseErrorKind::EmptyName);
        assert_eq!(
            kind("Click button:\"Open\" soon"),
            ParseErrorKind::BadDuration("soon".into())
        );
    }

    #[test]
    fn paste_takes_quoted_text() {
        assert_eq!(
            steps("Paste \"echo \\\"hi\\\"\""),
            [Step::Paste("echo \"hi\"".into())]
        );
        assert_eq!(kind("Paste echo"), ParseErrorKind::ExpectedQuote);
    }

    #[test]
    fn focus_names_an_app_with_an_optional_wait() {
        assert_eq!(
            steps("Focus \"Visual Studio Code\" 20s"),
            [Step::Focus {
                app: AppName("Visual Studio Code".into()),
                timeout: Duration::from_secs(20),
            }]
        );
        assert_eq!(
            steps("Focus \"Terminal\""),
            [Step::Focus {
                app: AppName("Terminal".into()),
                timeout: DEFAULT_WAIT,
            }]
        );
        assert_eq!(kind("Focus \"\""), ParseErrorKind::EmptyName);
        assert_eq!(kind("Focus Terminal"), ParseErrorKind::ExpectedQuote);
    }

    #[test]
    fn dock_names_cannot_break_out_of_applescript_strings() {
        assert_eq!(
            kind("Click dock:\"a\\\"b\""),
            ParseErrorKind::BadTarget("a\"b".into())
        );
        assert_eq!(
            kind("Click dock:\"a\\\\b\""),
            ParseErrorKind::BadTarget("a\\b".into())
        );
        assert_eq!(
            kind("Click dock:\"a\\nb\""),
            ParseErrorKind::BadTarget("a\nb".into())
        );
        assert_eq!(
            kind("Click dock:\"\""),
            ParseErrorKind::BadTarget(String::new())
        );
        assert_eq!(
            kind("Click dock:\"Finder\" x"),
            ParseErrorKind::TrailingInput("x".into())
        );
    }

    #[test]
    fn key_chords_use_cliclick_names() {
        let chord =
            |modifiers: Vec<Modifier>, key: Key| vec![Step::Key(KeyChord { modifiers, key })];
        assert_eq!(steps("Key return"), chord(vec![], Key::Named("return")));
        assert_eq!(
            steps("Key cmd+space"),
            chord(vec![Modifier::Cmd], Key::Named("space"))
        );
        assert_eq!(steps("Key f12"), chord(vec![], Key::Named("f12")));
        assert_eq!(
            steps("Key ctrl+alt+fn+arrow-left"),
            chord(
                vec![Modifier::Ctrl, Modifier::Alt, Modifier::Fn],
                Key::Named("arrow-left")
            )
        );
        assert_eq!(steps("Key a"), chord(vec![], Key::Char('a')));
        assert_eq!(
            steps("Key shift+1"),
            chord(vec![Modifier::Shift], Key::Char('1'))
        );
    }

    #[test]
    fn key_chords_expose_modifiers_and_key() {
        let Step::Key(chord) = &steps("Key ctrl+alt+fn+arrow-left")[0] else {
            panic!("not a key step");
        };
        assert_eq!(
            chord.modifiers(),
            [Modifier::Ctrl, Modifier::Alt, Modifier::Fn]
        );
        assert_eq!(chord.key(), &Key::Named("arrow-left"));
        assert_eq!(Modifier::Cmd.cliclick_name(), "cmd");
    }
}
