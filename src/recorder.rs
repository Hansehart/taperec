//! The recorder process, and video encoding with ffmpeg.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use crate::units::{self, Finer};

/// ffmpeg options that keep output to errors and overwrite the target.
pub(crate) const QUIET: [&str; 4] = ["-hide_banner", "-loglevel", "error", "-y"];

/// ffmpeg options for H.264 that plays everywhere.
pub(crate) const H264: [&str; 8] = [
    "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
];

/// ffmpeg options for an MP4 that streams from its first byte.
pub(crate) const MP4: [&str; 2] = ["-movflags", "+faststart"];

/// The metadata tag holding the clock time of a capture's first frame, in seconds.
pub const START_TAG: &str = "com.taperec.start";

/// How often a starting recorder's output file is checked.
const FILE_POLL: Duration = Duration::from_millis(50);
/// How long the recorder gets to exit after `q` before it is ended.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Recording frame rate.
pub const FPS: NonZeroU32 = NonZeroU32::MIN.saturating_add(29);

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

/// When the video at `path` starts: the clock time of its first frame, from its [`START_TAG`] or else
/// the container's start time.
///
/// # Errors
///
/// [`Error::Spawn`]/[`Error::Exit`] for a failing prober, [`Error::Probe`] for unexpected output.
pub fn probe_start(program: &str, path: &Path) -> Result<Duration, Error> {
    let entries = format!("format=start_time:format_tags={START_TAG}");
    let args = [
        "-v",
        "error",
        "-show_entries",
        &entries,
        "-of",
        "default=noprint_wrappers=1",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([path.as_os_str().to_owned()])
    .collect::<Vec<_>>();
    let output = run(program, &args)?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_start(&text).ok_or_else(|| Error::Probe(text.into_owned()))
}

/// The start in ffprobe's `key=value` lines: the tag `ScreenCaptureKit` captures carry, else `start_time`,
/// which ffmpeg's captures set to the wall clock.
fn parse_start(text: &str) -> Option<Duration> {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .and_then(|v| units::seconds(v.trim(), Finer::Truncate))
    };
    value(&format!("TAG:{START_TAG}")).or_else(|| value("start_time"))
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
        // The first lines say why, e.g. which filter ffmpeg rejected; later ones follow from them.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(6)
            .collect();
        Err(Error::Exit {
            program: program.to_owned(),
            code: output.status.code(),
            detail: lines.join(" | "),
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
    /// A recorder that did not report it was recording in time.
    NotReady {
        /// The program.
        program: String,
        /// How long it had.
        limit: Duration,
    },
    /// A program that exited with a failure.
    Exit {
        /// The program.
        program: String,
        /// Its exit code, or `None` if a signal ended it.
        code: Option<i32>,
        /// The end of what it wrote to stderr, where it was kept.
        detail: String,
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
            Self::NotReady { program, limit } => {
                write!(f, "{program} did not start recording within {limit:?}")
            }
            Self::Exit {
                program,
                code: Some(code),
                detail,
            } if detail.is_empty() => write!(f, "{program} exited with {code}"),
            Self::Exit {
                program,
                code: Some(code),
                detail,
            } => write!(f, "{program} exited with {code}: {detail}"),
            Self::Exit {
                program,
                code: None,
                ..
            } => write!(f, "{program} was killed"),
            Self::Probe(output) => {
                write!(
                    f,
                    "expected a {START_TAG} time from ffprobe, got {output:?}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// A running recorder that is killed when dropped before [`Recording::stop`].
#[derive(Debug)]
pub struct Recording {
    program: String,
    child: Option<Child>,
    /// The recorder's stdout, line by line.
    lines: Receiver<String>,
}

impl Recording {
    /// Starts `program args…` with stdin piped and its stdout lines read.
    ///
    /// # Errors
    ///
    /// [`Error::Spawn`] for a failed start.
    #[expect(
        clippy::disallowed_methods,
        reason = "the recorder module is the seam that runs the recorder"
    )]
    pub fn start(program: &str, args: &[OsString]) -> Result<Self, Error> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| Error::spawn(program, &e))?;
        let (send, lines) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    // A dropped receiver only means nobody waits any more.
                    let _ = send.send(line);
                }
            });
        }
        Ok(Self {
            program: program.to_owned(),
            child: Some(child),
            lines,
        })
    }

    /// Waits up to `limit` for the recorder to put data in `out`, its sign that frames are being written.
    ///
    /// # Errors
    ///
    /// [`Error::Exit`] if the recorder exits first, [`Error::NotReady`] past `limit`.
    #[expect(
        clippy::disallowed_methods,
        reason = "a starting recorder is a real process on the real clock"
    )]
    pub fn wait_for_file(&mut self, out: &Path, limit: Duration) -> Result<(), Error> {
        let deadline = Instant::now() + limit;
        loop {
            if fs::metadata(out).is_ok_and(|m| m.len() > 0) {
                return Ok(());
            }
            self.check()?;
            if Instant::now() >= deadline {
                return Err(Error::NotReady {
                    program: self.program.clone(),
                    limit,
                });
            }
            thread::sleep(FILE_POLL);
        }
    }

    /// Waits up to `limit` for the recorder to print `line`, its sign that frames are being written.
    ///
    /// # Errors
    ///
    /// [`Error::Exit`] if the recorder exits first, [`Error::NotReady`] past `limit`.
    pub fn wait_for(&mut self, line: &str, limit: Duration) -> Result<(), Error> {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(printed) if printed == line => return Ok(()),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Error::NotReady {
                        program: self.program.clone(),
                        limit,
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Stdout closes as the recorder exits; reap it for its status.
                    let status = self.child.as_mut().map(Child::wait);
                    return Err(Error::Exit {
                        program: self.program.clone(),
                        code: status.and_then(Result::ok).and_then(|s| s.code()),
                        detail: String::new(),
                    });
                }
            }
        }
    }

    /// Sends the quit key `q`, closes stdin and ends the recorder after a five-second grace.
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
            // A recorder that lingers past the grace has written what it can.
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
                detail: String::new(),
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
                detail: String::new(),
            }),
        }
    }

    /// Kills the recorder, leaving its partial file to inspect.
    pub fn abort(mut self) {
        self.kill();
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
    use std::path::PathBuf;

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
            detail: String::new(),
        }
    }

    /// A stand-in recorder that stores the first byte it reads into `path`.
    fn fake_ffmpeg(path: &Path) -> Recording {
        let script = format!("head -c1 > '{}'", path.display());
        Recording::start("sh", &sh(&script)).unwrap()
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
        let recording = Recording::start("sh", &sh("exec sleep 30")).unwrap();
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
        let recording = Recording::start("sh", &sh("head -c1 >/dev/null; exit 3")).unwrap();
        assert_eq!(recording.stop(), Err(exit(3)));
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "lets a real child process write before it is killed"
    )]
    fn abort_kills_and_keeps_the_partial_file() {
        let path = temp("abort");
        let script = format!("echo partial > '{}'; sleep 30", path.display());
        let recording = Recording::start("sh", &sh(&script)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        recording.abort();
        assert_eq!(fs::read_to_string(&path).unwrap(), "partial\n");
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
        let mut recording = Recording::start("sh", &sh("exit 4")).unwrap();
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
    fn the_start_tag_wins_over_the_container_start() {
        assert_eq!(
            parse_start("start_time=0.000000\nTAG:com.taperec.start=1792327838.634795\n"),
            Some(Duration::from_micros(1_792_327_838_634_795))
        );
        assert_eq!(
            parse_start("start_time=1792327838.500000\n"),
            Some(Duration::from_millis(1_792_327_838_500))
        );
        assert_eq!(parse_start("start_time=N/A\n"), None);
        assert_eq!(parse_start(""), None);
    }

    #[test]
    fn probe_rejects_output_that_is_not_a_time() {
        // `echo` prints its arguments back.
        let err = probe_start("echo", Path::new("/x.mkv")).unwrap_err();
        assert!(matches!(err, Error::Probe(_)), "{err:?}");
    }

    #[test]
    fn waits_until_the_recorder_reports_recording() {
        let mut recording =
            Recording::start("sh", &sh("echo warming; echo recording; cat > /dev/null")).unwrap();
        assert_eq!(
            recording.wait_for("recording", Duration::from_secs(5)),
            Ok(())
        );
        assert_eq!(recording.stop(), Ok(()));
    }

    #[test]
    fn a_recorder_that_never_reports_is_not_ready() {
        let mut recording = Recording::start("sh", &sh("sleep 5")).unwrap();
        assert_eq!(
            recording.wait_for("recording", Duration::from_millis(100)),
            Err(Error::NotReady {
                program: "sh".into(),
                limit: Duration::from_millis(100),
            })
        );
    }

    #[test]
    fn a_failing_program_says_why() {
        let out = temp("encode-why");
        let err = encode("sh", &sh("echo 'Invalid chroma radius' >&2; exit 3"), &out).unwrap_err();
        assert_eq!(err.to_string(), "sh exited with 3: Invalid chroma radius");
    }

    #[test]
    fn a_recorder_that_exits_before_recording_reports_its_code() {
        let mut recording = Recording::start("sh", &sh("echo starting; exit 3")).unwrap();
        assert_eq!(
            recording.wait_for("recording", Duration::from_secs(5)),
            Err(Error::Exit {
                program: "sh".into(),
                code: Some(3),
                detail: String::new(),
            })
        );
    }

    #[test]
    fn waits_until_the_recorder_writes_its_file() {
        let path = temp("writes");
        let _ = fs::remove_file(&path);
        let script = format!(
            "sleep 0.2; echo frames > '{}'; cat > /dev/null",
            path.display()
        );
        let mut recording = Recording::start("sh", &sh(&script)).unwrap();
        assert_eq!(
            recording.wait_for_file(&path, Duration::from_secs(5)),
            Ok(())
        );
        assert_eq!(recording.stop(), Ok(()));
    }

    #[test]
    fn a_recorder_that_writes_nothing_is_not_ready() {
        let path = temp("silent");
        let _ = fs::remove_file(&path);
        let mut recording = Recording::start("sh", &sh("sleep 5")).unwrap();
        assert_eq!(
            recording.wait_for_file(&path, Duration::from_millis(100)),
            Err(Error::NotReady {
                program: "sh".into(),
                limit: Duration::from_millis(100),
            })
        );
        let mut recording = Recording::start("sh", &sh("exit 3")).unwrap();
        assert_eq!(
            recording.wait_for_file(&path, Duration::from_secs(5)),
            Err(Error::Exit {
                program: "sh".into(),
                code: Some(3),
                detail: String::new(),
            })
        );
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let err = Recording::start("/nonexistent/ffmpeg", &[]).unwrap_err();
        assert!(matches!(err, Error::Spawn { .. }), "{err:?}");
    }
}
