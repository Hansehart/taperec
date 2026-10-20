//! Test doubles for [`Platform`] and [`Recorder`] that record typed calls on a shared virtual clock.

use std::cell::Cell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use super::{CaptureError, Control, Error, Platform, Recorder};
use crate::tape::{AppName, KeyChord, Point, ShellCommand};

/// What the player asked the platform to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    MoveTo(Point),
    Click,
    Key(String),
    Type(char),
    Paste(String),
    Windows,
    Buttons,
    Dock,
    Focus(String),
    Shell(String),
    Screenshot(String),
}

/// A platform that records typed calls, answers queries from queues and keeps virtual time.
#[derive(Default)]
pub(crate) struct FakePlatform {
    pub(crate) calls: Vec<Call>,
    /// Virtual time, shared with a [`FakeRecorder`] made by [`FakePlatform::recorder`].
    pub(crate) clock: Rc<Cell<Duration>>,
    /// Virtual time each query takes.
    pub(crate) query_cost: Duration,
    pub(crate) windows: VecDeque<Vec<String>>,
    pub(crate) buttons: VecDeque<Vec<Control>>,
    pub(crate) dock: Vec<(String, Point)>,
    pub(crate) focus: VecDeque<bool>,
    pub(crate) shell: VecDeque<bool>,
}

impl FakePlatform {
    /// Virtual time so far.
    pub(crate) fn now(&self) -> Duration {
        self.clock.get()
    }

    /// A recorder on this platform's clock.
    pub(crate) fn recorder(&self) -> FakeRecorder {
        FakeRecorder {
            clock: Rc::clone(&self.clock),
            ..FakeRecorder::default()
        }
    }

    fn advance(&self, duration: Duration) {
        self.clock.set(self.clock.get() + duration);
    }

    /// Spends one query's time, failing like a real query cut off at `limit`.
    fn query(&mut self, call: Call, limit: Option<Duration>) -> Result<(), Error> {
        self.calls.push(call);
        if let Some(limit) = limit.filter(|limit| self.query_cost > *limit) {
            self.advance(limit);
            return Err(Error::TimedOut {
                command: "query".into(),
            });
        }
        self.advance(self.query_cost);
        Ok(())
    }
}

impl Platform for FakePlatform {
    fn sleep(&mut self, duration: Duration) {
        self.advance(duration);
    }

    fn elapsed(&mut self) -> Duration {
        self.now()
    }

    fn pointer(&mut self) -> Result<Point, Error> {
        Ok(Point { x: 0, y: 0 })
    }

    fn move_to(&mut self, point: Point) -> Result<(), Error> {
        self.calls.push(Call::MoveTo(point));
        Ok(())
    }

    fn click(&mut self) -> Result<(), Error> {
        self.calls.push(Call::Click);
        Ok(())
    }

    fn key(&mut self, chord: &KeyChord) -> Result<(), Error> {
        self.calls.push(Call::Key(format!("{chord:?}")));
        Ok(())
    }

    fn type_char(&mut self, c: char) -> Result<(), Error> {
        self.calls.push(Call::Type(c));
        Ok(())
    }

    fn paste(&mut self, text: &str) -> Result<(), Error> {
        self.calls.push(Call::Paste(text.into()));
        Ok(())
    }

    fn windows(&mut self, limit: Option<Duration>) -> Result<Vec<String>, Error> {
        self.query(Call::Windows, limit)?;
        Ok(self.windows.pop_front().unwrap_or_default())
    }

    fn buttons(&mut self, limit: Option<Duration>) -> Result<Vec<Control>, Error> {
        self.query(Call::Buttons, limit)?;
        Ok(self.buttons.pop_front().unwrap_or_default())
    }

    fn dock(&mut self) -> Result<Vec<(String, Point)>, Error> {
        self.calls.push(Call::Dock);
        Ok(self.dock.clone())
    }

    fn focus(&mut self, app: &AppName, limit: Option<Duration>) -> Result<bool, Error> {
        self.query(Call::Focus(app.as_str().into()), limit)?;
        Ok(self.focus.pop_front().unwrap_or(false))
    }

    fn shell(&mut self, command: &ShellCommand, limit: Option<Duration>) -> Result<bool, Error> {
        self.query(Call::Shell(command.as_str().into()), limit)?;
        Ok(self.shell.pop_front().unwrap_or(false))
    }

    fn screenshot(&mut self, path: &Path) -> Result<(), Error> {
        self.calls
            .push(Call::Screenshot(path.display().to_string()));
        Ok(())
    }
}

/// What the record stage asked the recorder to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Capture {
    Started(PathBuf),
    Stopped,
    Aborted,
}

/// A recorder that notes its lifecycle and can die after a number of checks.
#[derive(Default)]
pub(crate) struct FakeRecorder {
    pub(crate) events: Vec<Capture>,
    clock: Rc<Cell<Duration>>,
    /// Checks that pass before capture dies on its own; `None` never dies.
    pub(crate) dies_after: Option<usize>,
    checks: usize,
}

impl Recorder for FakeRecorder {
    fn start(&mut self, out: &Path) -> Result<(), CaptureError> {
        self.events.push(Capture::Started(out.to_owned()));
        Ok(())
    }

    fn check(&mut self) -> Result<(), CaptureError> {
        self.checks += 1;
        match self.dies_after {
            Some(limit) if self.checks > limit => Err(CaptureError::Exit {
                program: "fake recorder".into(),
                code: Some(1),
            }),
            _ => Ok(()),
        }
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.events.push(Capture::Stopped);
        Ok(())
    }

    fn abort(&mut self) {
        self.events.push(Capture::Aborted);
    }

    fn clock(&self) -> Duration {
        self.clock.get()
    }

    fn raw_extension(&self) -> &'static str {
        "mkv"
    }
}
