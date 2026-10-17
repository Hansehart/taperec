//! What a desktop must offer to play tapes, and how it fails; one implementation per OS.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use crate::tape::{AppName, KeyChord, Point, ShellCommand};

#[cfg(test)]
pub(crate) mod fake;
pub mod macos;

/// What a desktop must offer to play tapes: input, what is on screen, a shell and a clock.
///
/// Queries take a `limit` bounding one attempt, so the waits built on them keep their deadlines.
pub trait Platform {
    /// Waits for `duration`.
    fn sleep(&mut self, duration: Duration);

    /// Monotonic time since the platform was created.
    fn elapsed(&mut self) -> Duration;

    /// Where the pointer is.
    ///
    /// # Errors
    ///
    /// Fails if the position cannot be read.
    fn pointer(&mut self) -> Result<Point, Error>;

    /// Moves the pointer to `point`.
    ///
    /// # Errors
    ///
    /// Fails if the move cannot be sent.
    fn move_to(&mut self, point: Point) -> Result<(), Error>;

    /// Left-clicks where the pointer is.
    ///
    /// # Errors
    ///
    /// Fails if the click cannot be sent.
    fn click(&mut self) -> Result<(), Error>;

    /// Presses a chord, holding its modifiers; `primary` means the platform's main shortcut modifier.
    ///
    /// # Errors
    ///
    /// Fails if the keys cannot be sent.
    fn key(&mut self, chord: &KeyChord) -> Result<(), Error>;

    /// Types one character, `\n` and `\t` as Return and Tab.
    ///
    /// # Errors
    ///
    /// [`Error::Untypable`] for a character outside the keyboard layout.
    fn type_char(&mut self, c: char) -> Result<(), Error>;

    /// Puts `text` on the clipboard and pastes it with one keystroke.
    ///
    /// # Errors
    ///
    /// Fails if the clipboard or the keystroke fails.
    fn paste(&mut self, text: &str) -> Result<(), Error>;

    /// The titles of visible windows.
    ///
    /// # Errors
    ///
    /// [`Error::TimedOut`] past `limit`, or a failed query.
    fn windows(&mut self, limit: Option<Duration>) -> Result<Vec<String>, Error>;

    /// The labelled buttons in visible windows.
    ///
    /// # Errors
    ///
    /// [`Error::TimedOut`] past `limit`, or a failed query.
    fn buttons(&mut self, limit: Option<Duration>) -> Result<Vec<Control>, Error>;

    /// Dock items by name and icon centre.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] on a platform without a Dock, or a failed query.
    fn dock(&mut self) -> Result<Vec<(String, Point)>, Error>;

    /// Brings `app` to the front, reporting whether it is there yet.
    ///
    /// # Errors
    ///
    /// [`Error::TimedOut`] past `limit`, or an app the platform cannot address.
    fn focus(&mut self, app: &AppName, limit: Option<Duration>) -> Result<bool, Error>;

    /// Runs `command` in the platform's shell, reporting whether it succeeded.
    ///
    /// # Errors
    ///
    /// [`Error::TimedOut`] past `limit`, or a shell that cannot start.
    fn shell(&mut self, command: &ShellCommand, limit: Option<Duration>) -> Result<bool, Error>;

    /// Saves the screen to `path` as PNG, silently.
    ///
    /// # Errors
    ///
    /// Fails if the capture fails.
    fn screenshot(&mut self, path: &Path) -> Result<(), Error>;
}

/// Captures the screen into a file whose frames carry [`Recorder::clock`] times; one implementation per OS.
pub trait Recorder {
    /// Starts capturing into `out`.
    ///
    /// # Errors
    ///
    /// Fails if capture cannot start.
    fn start(&mut self, out: &Path) -> Result<(), CaptureError>;

    /// Confirms capture is still running.
    ///
    /// # Errors
    ///
    /// Fails once capture has stopped on its own.
    fn check(&mut self) -> Result<(), CaptureError>;

    /// Ends capture, leaving a complete file.
    ///
    /// # Errors
    ///
    /// Fails if capture does not end cleanly.
    fn stop(&mut self) -> Result<(), CaptureError>;

    /// Ends capture at once, leaving the partial file to inspect.
    fn abort(&mut self);

    /// Now, on the clock the captured frames carry.
    fn clock(&self) -> Duration;
}

/// Why capture failed.
pub use crate::recorder::Error as CaptureError;

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
    /// A `button:` label that no window showed in time.
    ControlNotFound(String),
    /// A `button:` label shown by several apps, none of them frontmost.
    AmbiguousControl {
        /// The label.
        label: String,
        /// The apps showing it.
        apps: Vec<String>,
    },
    /// A `Focus` app that never came to the front.
    NotFrontmost(String),
    /// A platform extra this platform lacks, e.g. the Dock.
    NotSupported(&'static str),
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
            Self::ControlNotFound(label) => write!(f, "no button labelled {label:?} appeared"),
            Self::AmbiguousControl { label, apps } => write!(
                f,
                "a button labelled {label:?} shows in {}; bring one to the front with `Focus`",
                apps.join(", ")
            ),
            Self::NotFrontmost(app) => write!(f, "{app} did not come to the front"),
            Self::NotSupported(what) => write!(f, "this platform has no {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// A labelled button somewhere on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    /// The app whose window shows it.
    pub app: String,
    /// Whether that app is in front.
    pub frontmost: bool,
    /// Its label.
    pub label: String,
    /// The centre of its frame.
    pub centre: Point,
}

/// The centre of the button labelled `label`: the frontmost app's, or the only app's showing it.
///
/// # Errors
///
/// [`Error::AmbiguousControl`] when several apps show it and none is in front.
pub fn pick_control(controls: &[Control], label: &str) -> Result<Option<Point>, Error> {
    let matches: Vec<&Control> = controls.iter().filter(|c| c.label == label).collect();
    if let Some(front) = matches.iter().find(|c| c.frontmost) {
        return Ok(Some(front.centre));
    }
    let mut apps: Vec<String> = matches.iter().map(|c| c.app.clone()).collect();
    apps.sort();
    apps.dedup();
    match apps.len() {
        0 => Ok(None),
        1 => Ok(matches.first().map(|c| c.centre)),
        _ => Err(Error::AmbiguousControl {
            label: label.to_owned(),
            apps,
        }),
    }
}
