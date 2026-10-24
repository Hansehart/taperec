//! Ties parsing, desktop playback and recording together.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use crate::cli::Args;
use crate::cli::RecorderChoice;
use crate::desktop::Desktop;
use crate::platform::macos::{Capture, Helper, ScreenRecorder};
use crate::platform::{Platform, Recorder};
use crate::process::SystemRunner;
use crate::recorder::FPS;
use crate::{desktop, recorder, tape, warp};

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
    /// A drawn overlay image that cannot be put in place.
    Image {
        /// The image path.
        path: PathBuf,
        /// The I/O error.
        detail: String,
    },
    /// A timeline file that cannot be read or written.
    Timeline {
        /// The timeline path.
        path: PathBuf,
        /// What went wrong.
        detail: String,
    },
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
            Self::Timeline { path, detail } => {
                write!(f, "timeline {}: {detail}", path.display())
            }
            Self::Image { path, detail } => write!(f, "cannot place {}: {detail}", path.display()),
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

    let path = args.out_dir.join(tape.output.as_str());
    // The raw capture, timeline and captions stay next to the video, for `taperec render`.
    let stem = tape.output.as_str().trim_end_matches(".mp4");
    let mut helper = Helper::new(SystemRunner::default());
    // Software H.264 in VideoToolbox costs about three times libx264's CPU.
    let screencapturekit = match args.recorder {
        RecorderChoice::Ffmpeg => false,
        RecorderChoice::ScreenCaptureKit => true,
        RecorderChoice::Auto => helper.hardware_encoder()?,
    };
    let mut recorder = ScreenRecorder::new(if screencapturekit {
        Capture::ScreenCaptureKit
    } else {
        Capture::Ffmpeg
    });
    let video = format!("{stem}.raw.{}", recorder.raw_extension());
    let raw = args.out_dir.join(&video);
    let mut desktop = Desktop::new(helper);
    let marks = capture(
        &tape,
        &mut desktop,
        &mut recorder,
        &raw,
        args.failshot.as_deref(),
    )?;
    let timeline_path = args.out_dir.join(format!("{stem}.timeline.yaml"));
    save_timeline(&timeline_path, stem, &video, marks)?;
    render(&timeline_path, &path)?;
    Ok(path)
}

/// Plays `tape` on `desktop` while `recorder` captures it into `raw`, returning the warps and bar texts on the
/// capture's clock.
///
/// A failing step saves a screenshot to `failshot`, aborts capture and ends the recording.
///
/// # Errors
///
/// The first step or capture failure.
pub fn capture<P: Platform, R: Recorder>(
    tape: &tape::Tape,
    desktop: &mut Desktop<P>,
    recorder: &mut R,
    raw: &Path,
    failshot: Option<&str>,
) -> Result<warp::Marks, AppError> {
    recorder.start(raw)?;
    let mut timeline = warp::Timeline::default();
    for step in &tape.steps {
        // Marks share the clock the capture stamps its frames with.
        timeline.mark(step, recorder.clock());
        // A stopped recorder ends the step at once and explains the failure.
        let played = match step {
            // A zoom starts once its target is found, e.g. a button that appears later.
            tape::Step::Zoom(Some((target, scale))) => desktop
                .locate(target, &mut || recorder.check().is_ok())
                .map(|centre| timeline.zoom(Some((centre, scale.percent())), recorder.clock())),
            tape::Step::Zoom(None) => {
                timeline.zoom(None, recorder.clock());
                Ok(())
            }
            _ => desktop.play_while(step, &mut || recorder.check().is_ok()),
        };
        let result = recorder
            .check()
            .map_err(AppError::from)
            .and(played.map_err(AppError::from));
        if let Err(error) = result {
            // Best effort; the step's error is the one reported.
            if let Some(path) = failshot {
                let _ = desktop.screenshot(path);
            }
            recorder.abort();
            return Err(error);
        }
    }
    // A bar still showing lasts to the end of the capture.
    let marks = timeline.finish(recorder.clock());
    recorder.stop()?;
    Ok(marks)
}

/// Writes the timeline file naming the warps, bar texts, zooms and their image files; `render` draws the images.
fn save_timeline(path: &Path, stem: &str, video: &str, marks: warp::Marks) -> Result<(), AppError> {
    let warps = marks
        .warps
        .into_iter()
        .enumerate()
        .map(|(index, mark)| warp::WarpEntry {
            label: mark.label,
            start: mark.span.start,
            end: mark.span.end,
            length: mark.span.length,
            caption: format!("{stem}.warp{index}.png"),
        })
        .collect();
    let bars = marks
        .bars
        .into_iter()
        .enumerate()
        .map(|(index, bar)| warp::BarEntry {
            text: bar.text,
            start: bar.start,
            end: bar.end,
            image: format!("{stem}.bar{index}.png"),
        })
        .collect();
    let zooms = marks
        .zooms
        .into_iter()
        .map(|zoom| warp::ZoomEntry {
            at: zoom.at,
            x: zoom.x,
            y: zoom.y,
            percent: zoom.percent,
        })
        .collect();
    let file = warp::TimelineFile {
        video: video.to_owned(),
        warps,
        bars,
        zooms,
    };
    let timeline_error = |detail: String| AppError::Timeline {
        path: path.to_owned(),
        detail,
    };
    let text = serde_saphyr::to_string(&file).map_err(|e| timeline_error(e.to_string()))?;
    fs::write(path, text).map_err(|e| timeline_error(e.to_string()))
}

/// Builds the video at `out` from a recording's timeline file, its raw capture and captions; needs ffmpeg only.
///
/// # Errors
///
/// An unreadable timeline, or ffmpeg failing to probe, remux or render.
pub fn render(timeline: &Path, out: &Path) -> Result<(), AppError> {
    let timeline_error = |detail: String| AppError::Timeline {
        path: timeline.to_owned(),
        detail,
    };
    let text = fs::read_to_string(timeline).map_err(|e| timeline_error(e.to_string()))?;
    let file: warp::TimelineFile =
        serde_saphyr::from_str(&text).map_err(|e| timeline_error(e.to_string()))?;
    let dir = timeline.parent().unwrap_or(Path::new("."));
    let capture = dir.join(&file.video);
    // Warps shorter than a frame play as normal footage.
    let warps = warp::visible(file.warps, Duration::from_secs(1) / FPS.get());
    if warps.is_empty() && file.bars.is_empty() && file.zooms.is_empty() {
        return recorder::encode("ffmpeg", &recorder::remux_args(&capture, out), out)
            .map_err(AppError::Render);
    }
    // Overlays come from the timeline's texts, so an edited timeline renders its new texts.
    for warp in &warps {
        let text = warp::caption(&warp.label, warp.end.saturating_sub(warp.start));
        draw(
            CAPTION_TOOL,
            &[OsString::from(text)],
            &dir.join(&warp.caption),
        )?;
    }
    for bar in &file.bars {
        let args = [OsString::from("--bar"), OsString::from(&bar.text)];
        draw(CAPTION_TOOL, &args, &dir.join(&bar.image))?;
    }
    let start = recorder::probe_start("ffprobe", &capture)?;
    let zeroed = std::env::temp_dir().join(format!("taperec-{}-zeroed.mp4", process::id()));
    let rendered = (|| {
        recorder::encode("ffmpeg", &recorder::remux_args(&capture, &zeroed), &zeroed)
            .map_err(AppError::Render)?;
        let spans = warp::spans(&warps, start);
        let bars = warp::bar_spans(&file.bars, start);
        let zooms = warp::zoom_views(&file.zooms, start);
        // Warp captions first, then bar images, as the graph numbers its inputs.
        let images: Vec<PathBuf> = warps
            .iter()
            .map(|w| dir.join(&w.caption))
            .chain(file.bars.iter().map(|b| dir.join(&b.image)))
            .collect();
        let graph = warp::filter_graph(&spans, &bars, &zooms, FPS).map_err(AppError::Warp)?;
        let args = warp::render_args(&zeroed, &images, &graph, out);
        recorder::encode("ffmpeg", &args, out).map_err(AppError::Render)
    })();
    remove(&zeroed);
    rendered
}

/// Draws an overlay image with `program args… <image>`; without the program, e.g. off macOS, an image drawn
/// before is kept.
fn draw(program: &str, args: &[OsString], image: &Path) -> Result<(), AppError> {
    // Drawn aside, so a failure never removes an image drawn before.
    let fresh = image.with_extension("new.png");
    let mut args = args.to_vec();
    args.push(fresh.clone().into_os_string());
    match recorder::encode(program, &args, &fresh) {
        Ok(()) => fs::rename(&fresh, image).map_err(|e| AppError::Image {
            path: image.to_owned(),
            detail: e.to_string(),
        }),
        Err(recorder::Error::Spawn { .. }) if image.exists() => Ok(()),
        Err(error) => Err(AppError::Caption(error)),
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
            recorder: RecorderChoice::Auto,
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

    /// A `sh -c` tool drawing `script` with the image path as `$0`.
    fn tool(script: &str) -> [OsString; 2] {
        [OsString::from("-c"), OsString::from(script)]
    }

    fn image(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("taperec-draw-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("bar0.png")
    }

    #[test]
    fn draw_replaces_an_image_with_a_fresh_one() {
        let path = image("fresh");
        fs::write(&path, "old").unwrap();
        draw("sh", &tool("echo new > \"$0\""), &path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
    }

    #[test]
    fn without_the_tool_an_image_drawn_before_is_kept() {
        let path = image("kept");
        fs::write(&path, "drawn on a Mac").unwrap();
        draw("/nonexistent/taperec-caption", &[], &path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "drawn on a Mac");
    }

    #[test]
    fn without_the_tool_or_an_image_drawing_fails() {
        let path = image("missing");
        let _ = fs::remove_file(&path);
        let err = draw("/nonexistent/taperec-caption", &[], &path).unwrap_err();
        assert!(
            matches!(err, AppError::Caption(recorder::Error::Spawn { .. })),
            "{err:?}"
        );
    }

    #[test]
    fn a_failing_tool_leaves_the_image_drawn_before() {
        let path = image("failing");
        fs::write(&path, "old").unwrap();
        let err = draw("sh", &tool("exit 3"), &path).unwrap_err();
        assert!(
            matches!(err, AppError::Caption(recorder::Error::Exit { .. })),
            "{err:?}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn the_timeline_is_saved_without_drawing_anything() {
        let dir = std::env::temp_dir().join(format!("taperec-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let marks = warp::Marks {
            bars: vec![warp::MarkedBar {
                start: Duration::from_secs(1),
                end: Duration::from_secs(2),
                text: "Part 1".into(),
            }],
            ..warp::Marks::default()
        };
        let path = dir.join("t.timeline.yaml");
        save_timeline(&path, "t", "t.raw.mkv", marks).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("image: t.bar0.png"), "{text}");
        assert!(!dir.join("t.bar0.png").exists());
    }

    #[test]
    fn records_only_on_macos() {
        let path = tape_file("notmacos", "output: a.mp4\nsteps:\n  - sleep: 1s\n");
        assert_eq!(
            record(&args(path), "linux"),
            Err(AppError::NotMacOs("linux".into()))
        );
    }

    mod capture_stage {
        use std::collections::VecDeque;
        use std::time::Duration;

        use super::super::*;
        use crate::platform::fake::{Capture, FakePlatform};

        fn tape(items: &str) -> tape::Tape {
            tape::parse(&format!("output: t.mp4\nsteps:\n{items}")).unwrap()
        }

        #[test]
        fn marks_warps_on_the_capture_clock_between_start_and_stop() {
            let platform = FakePlatform::default();
            let mut recorder = platform.recorder();
            let mut desktop = Desktop::new(platform);
            let raw = Path::new("/out/t.raw.mov");
            let marks = capture(
                &tape("  - bar: Building\n  - zoom: { point: { x: 400, y: 300 }, scale: 2 }\n  - sleep: 2s\n  - warp: { label: Build, length: 5s, steps: [sleep: 10s] }\n  - zoom: off\n  - sleep: 1s\n"),
                &mut desktop,
                &mut recorder,
                raw,
                None,
            )
            .unwrap();
            assert_eq!(
                recorder.events,
                [Capture::Started(raw.to_owned()), Capture::Stopped]
            );
            let marked = &marks.warps;
            assert_eq!(marked.len(), 1);
            assert_eq!(marked[0].label, "Build");
            // The warp opens after the 2 s sleep and closes after its own 10 s.
            assert_eq!(marked[0].span.start, Duration::from_secs(2));
            assert_eq!(marked[0].span.end, Duration::from_secs(12));
            assert_eq!(marked[0].span.length, Duration::from_secs(5));
            // The zoom starts at once, onto its point, and ends after the warp.
            assert_eq!(
                marks.zooms,
                [
                    warp::MarkedZoom {
                        at: Duration::ZERO,
                        x: 400,
                        y: 300,
                        percent: 200,
                    },
                    warp::MarkedZoom {
                        at: Duration::from_secs(12),
                        x: 960,
                        y: 540,
                        percent: 100,
                    },
                ]
            );
            // The bar shows from the start until the capture ends after the last 1 s.
            assert_eq!(
                marks.bars,
                [warp::MarkedBar {
                    start: Duration::ZERO,
                    end: Duration::from_secs(13),
                    text: "Building".into(),
                }]
            );
        }

        #[test]
        fn a_failing_step_saves_the_failshot_and_aborts_capture() {
            let platform = FakePlatform::default();
            let mut recorder = platform.recorder();
            let mut desktop = Desktop::new(platform);
            let err = capture(
                &tape("  - click: { button: Open, timeout: 1s }\n  - sleep: 1s\n"),
                &mut desktop,
                &mut recorder,
                Path::new("/out/t.raw.mov"),
                Some("/out/t-failshot.png"),
            )
            .unwrap_err();
            assert_eq!(
                err,
                AppError::Desktop(desktop::Error::ControlNotFound("Open".into()))
            );
            assert_eq!(recorder.events.last(), Some(&Capture::Aborted));
            assert!(desktop.into_platform().calls.contains(
                &crate::platform::fake::Call::Screenshot("/out/t-failshot.png".into())
            ),);
        }

        #[test]
        fn a_recorder_that_dies_interrupts_the_tape() {
            let platform = FakePlatform {
                shell: VecDeque::new(),
                ..FakePlatform::default()
            };
            let mut recorder = platform.recorder();
            recorder.dies_after = Some(3);
            let mut desktop = Desktop::new(platform);
            let err = capture(
                &tape("  - waitUntil: { command: never, timeout: 10m }\n"),
                &mut desktop,
                &mut recorder,
                Path::new("/out/t.raw.mov"),
                None,
            )
            .unwrap_err();
            assert!(matches!(err, AppError::Record(_)), "{err:?}");
            assert_eq!(recorder.events.last(), Some(&Capture::Aborted));
        }

        #[test]
        fn a_tape_without_warps_or_bars_stops_cleanly_with_no_marks() {
            let platform = FakePlatform::default();
            let mut recorder = platform.recorder();
            let mut desktop = Desktop::new(platform);
            let marked = capture(
                &tape("  - paste: echo hi\n  - key: return\n"),
                &mut desktop,
                &mut recorder,
                Path::new("/out/t.raw.mov"),
                None,
            )
            .unwrap();
            assert_eq!(marked, warp::Marks::default());
            assert_eq!(recorder.events.last(), Some(&Capture::Stopped));
        }
    }
}
