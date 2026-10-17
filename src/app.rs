//! Ties parsing, desktop playback and recording together.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant};

use crate::cli::Args;
use crate::desktop::{Desktop, SystemRunner};
use crate::recorder::Recording;
use crate::{desktop, recorder, tape, warp};

/// Recording frame rate.
const FPS: NonZeroU32 = NonZeroU32::MIN.saturating_add(29);
/// The program that renders a caption to a PNG: `taperec-caption <text> <out.png>`.
const CAPTION_TOOL: &str = "taperec-caption";

/// Why a recording failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// An unreadable tape file.
    Read {
        /// The tape path.
        path: PathBuf,
        /// The I/O error.
        detail: String,
    },
    /// The tape is invalid.
    Parse {
        /// The tape path.
        path: PathBuf,
        /// What is wrong, and where.
        error: tape::ParseError,
    },
    /// A host other than macOS, by OS name.
    NotMacOs(String),
    /// A failure creating the output directory.
    OutDir {
        /// The directory.
        path: PathBuf,
        /// The I/O error.
        detail: String,
    },
    /// Driving the desktop failed.
    Desktop(desktop::Error),
    /// Recording failed.
    Record(recorder::Error),
    /// A failure building the warp graph.
    Warp(warp::Error),
    /// A failure rendering a warp caption image.
    Caption(recorder::Error),
    /// Rendering the warped video failed.
    Render(recorder::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, detail } => write!(f, "cannot read {}: {detail}", path.display()),
            // `path:line:column: message`, the shape editors and terminals link to.
            Self::Parse { path, error } => match &error.at {
                tape::Where::Position { line, column } => write!(
                    f,
                    "{}:{line}:{column}: {}",
                    path.display(),
                    error.message
                ),
                tape::Where::Step(_) | tape::Where::Document => {
                    write!(f, "{}: {error}", path.display())
                }
            },
            Self::NotMacOs(os) => write!(
                f,
                "taperec records the macOS desktop, not {os}; render tapes with the render workflow on GitHub Actions"
            ),
            Self::OutDir { path, detail } => {
                write!(f, "cannot create {}: {detail}", path.display())
            }
            Self::Desktop(error) => error.fmt(f),
            Self::Record(error) => error.fmt(f),
            Self::Warp(error) => error.fmt(f),
            Self::Caption(error) => write!(f, "rendering a warp caption failed: {error}"),
            Self::Render(error) => write!(f, "rendering warps failed: {error}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<desktop::Error> for AppError {
    fn from(error: desktop::Error) -> Self {
        Self::Desktop(error)
    }
}

impl From<recorder::Error> for AppError {
    fn from(error: recorder::Error) -> Self {
        Self::Record(error)
    }
}

/// Reads and parses each tape without recording, returning one error per invalid tape.
#[must_use]
pub fn check(tapes: &[PathBuf]) -> Vec<AppError> {
    tapes.iter().filter_map(|path| load(path).err()).collect()
}

/// Reads and parses the tape at `path`.
fn load(path: &Path) -> Result<tape::Tape, AppError> {
    let source = fs::read_to_string(path).map_err(|e| AppError::Read {
        path: path.to_owned(),
        detail: e.to_string(),
    })?;
    tape::parse(&source).map_err(|error| AppError::Parse {
        path: path.to_owned(),
        error,
    })
}

/// Plays `args.tape` while recording it and returns the video path.
///
/// # Errors
///
/// Fails before recording on a bad tape or a non-macOS host; a failed step aborts the recording.
pub fn record(args: &Args, os: &str) -> Result<PathBuf, AppError> {
    let tape = load(&args.tape)?;
    if os != "macos" {
        return Err(AppError::NotMacOs(os.to_owned()));
    }
    fs::create_dir_all(&args.out_dir).map_err(|e| AppError::OutDir {
        path: args.out_dir.clone(),
        detail: e.to_string(),
    })?;

    let mut desktop = Desktop::new(SystemRunner::default());
    let path = args.out_dir.join(tape.output.as_str());
    let warped = tape
        .steps
        .iter()
        .any(|s| matches!(s, tape::Step::Warp { .. }));
    // The capture streams into a hidden file and is finished into `path` afterwards.
    let capture_path = args
        .out_dir
        .join(format!(".{}.capture.ts", tape.output.as_str()));
    let ffmpeg = recorder::ffmpeg_args(FPS, &capture_path);
    let mut recording = Recording::start("ffmpeg", &ffmpeg, capture_path.clone())?;
    let started = Instant::now();
    let mut timeline = warp::Timeline::default();
    for step in &tape.steps {
        timeline.mark(step, started.elapsed());
        // A stopped recorder ends the step at once and explains the failure.
        let played = desktop.play_while(step, &mut || recording.check().is_ok());
        let result = recording
            .check()
            .map_err(AppError::from)
            .and(played.map_err(AppError::from));
        if let Err(error) = result {
            // Best effort; the step's error is the one reported.
            if let Some(path) = args.failshot.as_deref() {
                let _ = desktop.screenshot(path);
            }
            recording.abort();
            return Err(error);
        }
    }
    let stopped_at = started.elapsed();
    recording.stop()?;
    let marked = warped.then(|| timeline.into_marked());
    let finished = finish(&capture_path, marked, stopped_at, &path);
    keep_on_error(&finished, &capture_path);
    finished?;
    Ok(path)
}

/// Turns a capture into the video at `out`, applying warps marked on the tape's clock.
fn finish(
    capture: &Path,
    marked: Option<Vec<warp::Marked>>,
    stopped_at: Duration,
    out: &Path,
) -> Result<(), AppError> {
    let Some(marked) = marked else {
        return recorder::encode("ffmpeg", &recorder::remux_args(capture, out), out)
            .map_err(AppError::Render);
    };
    let zeroed = capture.with_extension("mp4");
    recorder::encode("ffmpeg", &recorder::remux_args(capture, &zeroed), &zeroed)
        .map_err(AppError::Render)?;
    // Frames carry host-clock times, so the capture ends at `stopped_at`.
    let captured = recorder::probe_duration("ffprobe", &zeroed)?;
    let aligned = warp::align(marked, stopped_at.saturating_sub(captured));
    // Warps shorter than a frame play as normal footage.
    let visible = warp::visible(aligned, Duration::from_secs(1) / FPS.get());
    let rendered = render_warps(&zeroed, visible, out);
    keep_on_error(&rendered, &zeroed);
    rendered
}

/// Renders `capture` with `marked` warps applied into `out`.
fn render_warps(capture: &Path, marked: Vec<warp::Marked>, out: &Path) -> Result<(), AppError> {
    let mut captions = Vec::with_capacity(marked.len());
    let rendered = (|| {
        let mut spans = Vec::with_capacity(marked.len());
        for (index, mark) in marked.into_iter().enumerate() {
            let image =
                std::env::temp_dir().join(format!("taperec-{}-warp{index}.png", process::id()));
            let text = warp::caption(&mark.label, mark.span.end.saturating_sub(mark.span.start));
            captions.push(image.clone());
            let args = [OsString::from(text), image.clone().into_os_string()];
            recorder::encode(CAPTION_TOOL, &args, &image).map_err(AppError::Caption)?;
            spans.push(mark.span);
        }
        let graph = warp::filter_graph(&spans, FPS).map_err(AppError::Warp)?;
        let args = warp::render_args(capture, &captions, &graph, out);
        recorder::encode("ffmpeg", &args, out).map_err(AppError::Render)
    })();
    for image in &captions {
        remove(image);
    }
    rendered
}

/// Deletes an intermediate file once its step succeeded, keeping it to inspect otherwise.
fn keep_on_error<T, E>(result: &Result<T, E>, path: &Path) {
    if result.is_ok() {
        remove(path);
    }
}

/// Deletes an intermediate file, best effort.
fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn args(tape: PathBuf) -> Args {
        Args {
            failshot: None,
            tape,
            out_dir: std::env::temp_dir(),
        }
    }

    fn tape_file(name: &str, content: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("taperec-app-{}-{name}.yaml", std::process::id()));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn missing_tape_is_a_read_error() {
        let path = PathBuf::from("/nonexistent/demo.yaml");
        let err = record(&args(path.clone()), "macos").unwrap_err();
        assert!(
            matches!(&err, AppError::Read { path: p, .. } if *p == path),
            "{err:?}"
        );
    }

    #[test]
    fn invalid_tape_is_a_parse_error_with_its_position() {
        let path = tape_file("invalid", "output: a.mp4\nsteps:\n  - jump: 3\n");
        let err = record(&args(path.clone()), "macos").unwrap_err();
        assert!(
            matches!(
                &err,
                AppError::Parse {
                    error: tape::ParseError {
                        at: tape::Where::Position { line: 3, .. },
                        ..
                    },
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(
            err.to_string()
                .starts_with(&format!("{}:3:", path.display())),
            "{err}"
        );
    }

    #[test]
    fn check_reports_each_invalid_tape() {
        let good = tape_file("good", "output: a.mp4\nsteps:\n  - sleep: 1s\n");
        let bad = tape_file("bad", "output: a.mp4\nsteps:\n  - sleep: 5x\n");
        let errors = check(&[good, bad.clone()]);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            matches!(&errors[0], AppError::Parse { path, .. } if *path == bad),
            "{errors:?}"
        );
    }

    #[test]
    fn records_only_on_macos() {
        let path = tape_file("notmacos", "output: a.mp4\nsteps:\n  - sleep: 1s\n");
        assert_eq!(
            record(&args(path), "linux"),
            Err(AppError::NotMacOs("linux".into()))
        );
    }
}
