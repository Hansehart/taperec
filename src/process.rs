//! Running child processes within a time limit, behind a seam tests replace.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{self, Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use crate::platform::Error;

/// How often a waiting caller checks whether its child has exited.
const CHECK_INTERVAL: Duration = Duration::from_millis(10);

/// Waits for `child` to exit, returning `None` once `limit` passes first.
#[expect(
    clippy::disallowed_methods,
    reason = "the process seam paces its own checks on children"
)]
pub(crate) fn wait_within(
    child: &mut Child,
    limit: Option<Duration>,
) -> io::Result<Option<ExitStatus>> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            return Ok(None);
        }
        thread::sleep(CHECK_INTERVAL);
    }
}

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
        let waited = wait_within(&mut child, limit).map_err(|e| Error::Spawn {
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

/// `program args…` as one line, for messages and logs.
pub(crate) fn command_line(program: &str, args: &[&str]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
