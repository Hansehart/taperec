//! Screen recording and video encoding with ffmpeg.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use crate::units::{self, Finer};

/// ffmpeg options that keep output to errors and overwrite the target.
pub(crate) const QUIET: [&str; 4] = ["-hide_banner", "-loglevel", "error", "-y"];

/// ffmpeg options for H.264 that plays everywhere.
pub(crate) const H264: [&str; 8] = [
    "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
];

/// ffmpeg options for an MP4 that streams from its first byte.
pub(crate) const MP4: [&str; 2] = ["-movflags", "+faststart"];

/// How long the recorder gets to exit after `q` before it is ended.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// The main display as ffmpeg's avfoundation input lists it.
const SCREEN: &str = "Capture screen 0";

/// ffmpeg arguments that record the screen, cursor and clicks at `fps` into a Matroska `out`.
///
/// Frames carry wall-clock times, the clock the timeline marks warps on.
#[must_use]
pub fn ffmpeg_args(fps: NonZeroU32, out: &Path) -> Vec<OsString> {
    let fps = fps.to_string();
    #[rustfmt::skip]
    let capture = [
        // ffmpeg reads `q` once per stats period, here every 50 ms.
        "-stats_period", "0.05",
        "-use_wallclock_as_timestamps", "1",
        "-f", "avfoundation", "-capture_cursor", "1", "-capture_mouse_clicks", "1",
        "-pixel_format", "bgr0", "-framerate", &fps, "-i", SCREEN,
    ];
    // Matroska keeps the wall-clock times whole; written live, one-second clusters reach the disk
    // as they close, so a killed recorder loses under a second.
    #[rustfmt::skip]
    let stream = [
        "-tune", "zerolatency", "-copyts", "-flush_packets", "1",
        "-f", "matroska", "-live", "1", "-cluster_time_limit", "1000",
    ];
    QUIET
        .into_iter()
        .chain(capture)
        .chain(H264)
        .chain(stream)
        .map(OsString::from)
        .chain([out.as_os_str().to_owned()])
        .collect()
}

/// ffmpeg arguments that copy a capture into an MP4 starting at time zero.
#[must_use]
pub fn remux_args(capture: &Path, out: &Path) -> Vec<OsString> {
    let copy = ["-c", "copy", "-avoid_negative_ts", "make_zero"];
    QUIET
        .map(OsString::from)
        .into_iter()
        .chain([OsString::from("-i"), capture.as_os_str().to_owned()])
        .chain(copy.map(OsString::from))
        .chain(MP4.map(OsString::from))
        .chain([out.as_os_str().to_owned()])
        .collect()
}

/// Runs `program args…` to produce `out`, removing a partial `out` on failure.
///
/// # Errors
///
/// [`Error::Spawn`] for a failed start, [`Error::Exit`] for a failing exit status.
pub fn encode(program: &str, args: &[OsString], out: &Path) -> Result<(), Error> {
    run(program, args).map(drop).inspect_err(|_| {
        // Removal is best effort.
        let _ = fs::remove_file(out);
    })
}

/// When the video at `path` starts, from the container's start time: the wall-clock time of its first frame.
///
/// # Errors
///
/// [`Error::Spawn`]/[`Error::Exit`] for a failing prober, [`Error::Probe`] for unexpected output.
pub fn probe_start(program: &str, path: &Path) -> Result<Duration, Error> {
    let args = [
        "-v",
        "error",
        "-show_entries",
        "format=start_time",
        "-of",
        "csv=p=0",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([path.as_os_str().to_owned()])
    .collect::<Vec<_>>();
    let output = run(program, &args)?;
    let text = String::from_utf8_lossy(&output.stdout);
    units::seconds(text.trim(), Finer::Truncate).ok_or_else(|| Error::Probe(text.into_owned()))
}

/// Runs `program args…` to completion with empty stdin, requiring success.
#[expect(
    clippy::disallowed_methods,
    reason = "the recorder module is the seam that runs ffmpeg and its helpers"
)]
fn run(program: &str, args: &[OsString]) -> Result<Output, Error> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::spawn(program, &e))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(Error::Exit {
            program: program.to_owned(),
            code: output.status.code(),
        })
    }
}

/// Why recording or encoding failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A program that failed to start.
    Spawn {
        /// The program.
        program: String,
        /// The I/O error.
        detail: String,
    },
    /// A failed attempt to stop the recorder.
    Stop {
        /// The program.
        program: String,
        /// The I/O error.
        detail: String,
    },
    /// A program that exited with a failure.
    Exit {
        /// The program.
        program: String,
        /// Its exit code, or `None` if a signal ended it.
        code: Option<i32>,
    },
    /// Prober output in an unexpected shape.
    Probe(String),
}

impl Error {
    fn spawn(program: &str, error: &std::io::Error) -> Self {
        Self::Spawn {
            program: program.to_owned(),
            detail: error.to_string(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { program, detail } => write!(f, "could not start {program}: {detail}"),
            Self::Stop { program, detail } => write!(f, "could not stop {program}: {detail}"),
            Self::Exit {
                program,
                code: Some(code),
            } => write!(f, "{program} exited with {code}"),
            Self::Exit {
                program,
                code: None,
            } => write!(f, "{program} was killed"),
            Self::Probe(output) => write!(f, "expected a duration from ffprobe, got {output:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// A running recorder that is killed when dropped before [`Recording::stop`].
#[derive(Debug)]
pub struct Recording {
    program: String,
    child: Option<Child>,
    path: PathBuf,
}

impl Recording {
    /// Starts `program args…`, which writes to `path`, with stdin piped.
    ///
    /// # Errors
    ///
    /// [`Error::Spawn`] for a failed start.
    #[expect(
        clippy::disallowed_methods,
        reason = "the recorder module is the seam that runs ffmpeg"
    )]
    pub fn start(program: &str, args: &[OsString], path: PathBuf) -> Result<Self, Error> {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| Error::spawn(program, &e))?;
        Ok(Self {
            program: program.to_owned(),
            child: Some(child),
            path,
        })
    }

    /// Sends ffmpeg's quit key `q` and ends the recorder after a five-second grace.
    ///
    /// # Errors
    ///
    /// [`Error::Stop`] for a failed `q`, [`Error::Exit`] for a failing exit status.
    pub fn stop(self) -> Result<(), Error> {
        self.stop_within(STOP_GRACE)
    }

    /// Stops like [`Recording::stop`] with `grace` as the time to exit.
    ///
    /// # Errors
    ///
    /// As [`Recording::stop`].
    pub fn stop_within(mut self, grace: Duration) -> Result<(), Error> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        // Closing stdin after `q` also ends a reader blocked on it.
        let sent = child
            .stdin
            .take()
            .ok_or_else(|| "stdin is not piped".to_owned())
            .and_then(|mut stdin| stdin.write_all(b"q").map_err(|e| e.to_string()));
        let waited = crate::process::wait_within(&mut child, Some(grace))
            .map_err(|e| self.stop_error(e.to_string()))?;
        let Some(status) = waited else {
            // macOS screen capture can stall in shutdown; the stream is complete by now.
            let _ = child.kill();
            let _ = child.wait();
            return sent.map_err(|detail| self.stop_error(detail));
        };
        sent.map_err(|detail| self.stop_error(detail))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Exit {
                program: self.program.clone(),
                code: status.code(),
            })
        }
    }

    /// Confirms the recorder is still running.
    ///
    /// # Errors
    ///
    /// [`Error::Exit`] with the recorder's exit code once it has exited.
    pub fn check(&mut self) -> Result<(), Error> {
        let status = match self.child.as_mut().map(Child::try_wait) {
            Some(Ok(status)) => status,
            Some(Err(e)) => return Err(self.stop_error(e.to_string())),
            None => return Ok(()),
        };
        match status {
            None => Ok(()),
            Some(status) => Err(Error::Exit {
                program: self.program.clone(),
                code: status.code(),
            }),
        }
    }

    /// Kills the recorder and removes its partial file.
    pub fn abort(mut self) {
        self.kill();
        // Removal is best effort.
        let _ = fs::remove_file(&self.path);
    }

    fn stop_error(&self, detail: String) -> Error {
        Error::Stop {
            program: self.program.clone(),
            detail,
        }
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Kill and reap the child, running or exited.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("taperec-test-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("out")
    }

    /// Arguments that make `sh` run `script`.
    fn sh(script: &str) -> [OsString; 2] {
        ["-c".into(), script.into()]
    }

    fn exit(code: i32) -> Error {
        Error::Exit {
            program: "sh".into(),
            code: Some(code),
        }
    }

    /// A stand-in recorder that stores the first byte it reads into `path`.
    fn fake_ffmpeg(path: &Path) -> Recording {
        let script = format!("head -c1 > '{}'", path.display());
        Recording::start("sh", &sh(&script), path.to_owned()).unwrap()
    }

    #[test]
    fn records_the_macos_screen_as_h264_on_the_wall_clock() {
        #[rustfmt::skip]
        let expected = [
            "-hide_banner", "-loglevel", "error", "-y",
            "-stats_period", "0.05",
            "-use_wallclock_as_timestamps", "1",
            "-f", "avfoundation", "-capture_cursor", "1", "-capture_mouse_clicks", "1",
            "-pixel_format", "bgr0", "-framerate", "30", "-i", "Capture screen 0",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
            "-tune", "zerolatency", "-copyts", "-flush_packets", "1",
            "-f", "matroska", "-live", "1", "-cluster_time_limit", "1000",
            "/out/demo.raw.mkv",
        ];
        let args = ffmpeg_args(NonZeroU32::new(30).unwrap(), Path::new("/out/demo.raw.mkv"));
        assert_eq!(strings(&args), expected);
    }

    #[test]
    fn remuxes_into_mp4_starting_at_zero() {
        #[rustfmt::skip]
        let expected = [
            "-hide_banner", "-loglevel", "error", "-y",
            "-i", "/out/demo.ts", "-c", "copy", "-avoid_negative_ts", "make_zero",
            "-movflags", "+faststart",
            "/out/demo.mp4",
        ];
        let args = remux_args(Path::new("/out/demo.ts"), Path::new("/out/demo.mp4"));
        assert_eq!(strings(&args), expected);
    }

    #[test]
    fn stop_sends_q_and_waits() {
        let path = temp("stop");
        assert_eq!(fake_ffmpeg(&path).stop(), Ok(()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "q");
    }

    #[test]
    fn stop_ends_a_recorder_that_lingers_after_q() {
        let recording = Recording::start("sh", &sh("exec sleep 30"), temp("stuck")).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(recording.stop_within(Duration::from_millis(200)), Ok(()));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn stop_reports_failed_exit() {
        let recording =
            Recording::start("sh", &sh("head -c1 >/dev/null; exit 3"), temp("exit")).unwrap();
        assert_eq!(recording.stop(), Err(exit(3)));
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "lets a real child process write before it is killed"
    )]
    fn abort_kills_and_deletes_partial_file() {
        let path = temp("abort");
        let script = format!("echo partial > '{}'; sleep 30", path.display());
        let recording = Recording::start("sh", &sh(&script), path.clone()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        recording.abort();
        assert!(!path.exists());
    }

    #[test]
    fn check_passes_while_running() {
        let mut recording = fake_ffmpeg(&temp("running"));
        assert_eq!(recording.check(), Ok(()));
        recording.stop().unwrap();
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "waits for a real child process to exit"
    )]
    fn check_reports_early_exit() {
        let mut recording = Recording::start("sh", &sh("exit 4"), temp("early")).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(recording.check(), Err(exit(4)));
    }

    #[test]
    fn encode_writes_output() {
        let out = temp("encode-ok");
        let script = format!("echo video > '{}'", out.display());
        assert_eq!(encode("sh", &sh(&script), &out), Ok(()));
        assert_eq!(fs::read_to_string(&out).unwrap(), "video\n");
    }

    #[test]
    fn encode_removes_partial_output_on_failure() {
        let out = temp("encode-fail");
        let script = format!("echo partial > '{}'; exit 3", out.display());
        assert_eq!(encode("sh", &sh(&script), &out), Err(exit(3)));
        assert!(!out.exists());
    }

    #[test]
    fn errors_name_the_program_that_failed() {
        let err = encode("/nonexistent/taperec-caption", &[], &temp("named")).unwrap_err();
        assert!(
            err.to_string().contains("/nonexistent/taperec-caption"),
            "{err}"
        );
    }

    #[test]
    fn encode_reports_missing_program() {
        let err = encode("/nonexistent/ffmpeg", &[], &temp("encode-spawn")).unwrap_err();
        assert!(matches!(err, Error::Spawn { .. }), "{err:?}");
    }

    #[test]
    fn probe_rejects_output_that_is_not_a_time() {
        // `echo` prints its arguments back.
        let err = probe_start("echo", Path::new("/x.mkv")).unwrap_err();
        assert!(matches!(err, Error::Probe(_)), "{err:?}");
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let err = Recording::start("/nonexistent/ffmpeg", &[], temp("spawn")).unwrap_err();
        assert!(matches!(err, Error::Spawn { .. }), "{err:?}");
    }
}
