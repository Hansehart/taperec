//! Waiting on child processes within a time limit.

use std::io;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

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
