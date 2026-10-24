//! Ties parsing, desktop playback and recording together.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use crate::cli::Args;
use crate::cli::RecorderChoice;
use crate::desktop::Desktop;
use crate::platform::macos::{Capture, Helper, ScreenRecorder};
use crate::platform::{Platform, Recorder, Shortcut};
use crate::process::SystemRunner;
use crate::recorder::FPS;
use crate::{desktop, edit, overlay, recorder, scene, tape, warp};

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
    /// The tape or edit file is invalid.
    Parse {
        /// The file's path.
        path: PathBuf,
        /// What is wrong, and where.
        error: tape::ParseError,
    },
    /// A host other than macOS, by OS name.
    NotMacOs(String),
    /// A directory that cannot be created: the output directory, or one for render's intermediate files.
    CreateDir {
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
    /// Drawing an overlay image failed.
    Overlay(overlay::Error),
    /// Rendering the warped video failed.
    Render(recorder::Error),

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
            Self::CreateDir { path, detail } => {
                write!(f, "cannot create {}: {detail}", path.display())
            }
            Self::Desktop(error) => error.fmt(f),
            Self::Record(error) => error.fmt(f),
            Self::Warp(error) => error.fmt(f),
            Self::Overlay(error) => error.fmt(f),
            Self::Render(error) => write!(f, "rendering warps failed: {error}"),
            Self::Timeline { path, detail } => {
                write!(f, "timeline {}: {detail}", path.display())
            }
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

/// Reads and parses each tape and its edit file without recording, returning one error per invalid tape.
#[must_use]
pub fn check(tapes: &[PathBuf]) -> Vec<AppError> {
    tapes
        .iter()
        .filter_map(|path| load(path).and_then(|tape| load_edit(path, &tape)).err())
        .collect()
}

/// Reads and parses the tape at `path`.
fn load(path: &Path) -> Result<tape::Tape, AppError> {
    parse_file(path, tape::parse)
}

/// Reads the edit file the tape at `path` names, if any, and checks it against the tape's marks; returns its
/// path.
fn load_edit(path: &Path, tape: &tape::Tape) -> Result<Option<PathBuf>, AppError> {
    let Some(name) = &tape.edit else {
        return Ok(None);
    };
    let edit_path = path.parent().unwrap_or(Path::new(".")).join(name);
    let edit = parse_file(&edit_path, edit::parse)?;
    edit::check(&edit, tape).map_err(|error| AppError::Parse {
        path: edit_path.clone(),
        error,
    })?;
    Ok(Some(edit_path))
}

/// Reads the file at `path` and parses it with `parse`.
fn parse_file<T>(
    path: &Path,
    parse: fn(&str) -> Result<T, tape::ParseError>,
) -> Result<T, AppError> {
    let source = fs::read_to_string(path).map_err(|e| AppError::Read {
        path: path.to_owned(),
        detail: e.to_string(),
    })?;
    parse(&source).map_err(|error| AppError::Parse {
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
    let edit = load_edit(&args.tape, &tape)?;
    if os != "macos" {
        return Err(AppError::NotMacOs(os.to_owned()));
    }
    fs::create_dir_all(&args.out_dir).map_err(|e| AppError::CreateDir {
        path: args.out_dir.clone(),
        detail: e.to_string(),
    })?;

    let path = args.out_dir.join(tape.output.as_str());
    // The raw capture and timeline stay next to the video, for `taperec render`.
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
    save_timeline(&timeline_path, &video, marks)?;
    render(&timeline_path, &path, edit.as_deref())?;
    Ok(path)
}

/// Plays `tape` on `desktop` while `recorder` captures it into `raw`, returning the warps and texts on the
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
    timeline.moment(tape::MarkName::START, recorder.clock(), None);
    for step in &tape.steps {
        // Marks share the clock the capture stamps its frames with.
        let at = recorder.clock();
        timeline.mark(step, at);
        let shortcut = match step {
            tape::Step::Key(chord) => Some(Shortcut::Chord(chord)),
            tape::Step::Paste(_) => Some(Shortcut::Paste),
            _ => None,
        };
        if let Some(shortcut) = shortcut {
            timeline.key(desktop.shortcut_labels(shortcut), at);
        }
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
            // A mark with a target falls once the target is found, like a zoom.
            tape::Step::Mark {
                name,
                at: Some(target),
            } => desktop
                .locate(target, &mut || recorder.check().is_ok())
                .map(|found| timeline.moment(name.as_str(), recorder.clock(), Some(found))),
            tape::Step::Mark { name, at: None } => {
                timeline.moment(name.as_str(), recorder.clock(), None);
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
    // A text still showing lasts to the end of the capture.
    let marks = timeline.finish(recorder.clock());
    recorder.stop()?;
    Ok(marks)
}

/// Writes the timeline file: what capture saw, for `render` to build the video from.
fn save_timeline(path: &Path, video: &str, marks: warp::Marks) -> Result<(), AppError> {
    let warps = marks
        .warps
        .into_iter()
        .map(|mark| warp::WarpEntry {
            label: mark.label,
            start: mark.span.start,
            end: mark.span.end,
            length: mark.span.length,
        })
        .collect();
    let texts = marks
        .texts
        .into_iter()
        .map(|text| warp::TextEntry {
            text: text.text,
            start: text.start,
            end: text.end,
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
    let moments = marks
        .moments
        .into_iter()
        .map(|moment| warp::MarkEntry {
            name: moment.name,
            at: moment.at,
            x: moment.position.map(|p| p.x),
            y: moment.position.map(|p| p.y),
        })
        .collect();
    let keys = marks
        .keys
        .into_iter()
        .map(|key| warp::KeyEntry {
            at: key.at,
            labels: key.labels,
        })
        .collect();
    let file = warp::TimelineFile {
        video: video.to_owned(),
        warps,
        texts,
        zooms,
        marks: moments,
        keys,
    };
    let timeline_error = |detail: String| AppError::Timeline {
        path: path.to_owned(),
        detail,
    };
    let text = serde_saphyr::to_string(&file).map_err(|e| timeline_error(e.to_string()))?;
    fs::write(path, text).map_err(|e| timeline_error(e.to_string()))
}

/// Builds the video at `out` from a recording's timeline file and raw capture, with the `edit` file on top if
/// given; draws its overlays itself and needs only ffmpeg, on any machine.
///
/// # Errors
///
/// An unreadable timeline or edit file, an edit not fitting the recording's marks, or ffmpeg failing to probe,
/// remux or render.
pub fn render(timeline: &Path, out: &Path, edit: Option<&Path>) -> Result<(), AppError> {
    let timeline_error = |detail: String| AppError::Timeline {
        path: timeline.to_owned(),
        detail,
    };
    let text = fs::read_to_string(timeline).map_err(|e| timeline_error(e.to_string()))?;
    let mut file: warp::TimelineFile =
        serde_saphyr::from_str(&text).map_err(|e| timeline_error(e.to_string()))?;
    let mut layout = edit::Layout::default();
    if let Some(path) = edit {
        let parse_error = |error| AppError::Parse {
            path: path.to_owned(),
            error,
        };
        let edit = parse_file(path, edit::parse)?;
        file = edit::apply(&edit, file).map_err(parse_error)?;
        layout = edit.layout();
    }
    let dir = timeline.parent().unwrap_or(Path::new("."));
    let capture = dir.join(&file.video);
    // Warps shorter than a frame play as normal footage.
    let warps = warp::visible(file.warps.clone(), Duration::from_secs(1) / FPS.get());
    let nothing = file.texts.is_empty() && file.keys.is_empty() && file.zooms.is_empty();
    if warps.is_empty() && nothing {
        return recorder::encode("ffmpeg", &recorder::remux_args(&capture, out), out)
            .map_err(AppError::Render);
    }
    let start = recorder::probe_start("ffprobe", &capture)?;
    let spans = warp::spans(&warps, start);
    let elements = scene::elements(&file, &warps, &spans, start, &layout);
    let frames = scene::frames(&elements, Duration::from_secs(1) / FPS.get());
    // Overlays and the zeroed capture are drawn fresh for each render and go with it.
    let scratch = std::env::temp_dir().join(format!("taperec-render-{}", process::id()));
    fs::create_dir_all(&scratch).map_err(|e| AppError::CreateDir {
        path: scratch.clone(),
        detail: e.to_string(),
    })?;
    let rendered = (|| {
        let overlays = if frames.is_empty() {
            None
        } else {
            Some(draw_overlays(&frames, &scratch)?)
        };
        let zeroed = scratch.join("zeroed.mp4");
        recorder::encode("ffmpeg", &recorder::remux_args(&capture, &zeroed), &zeroed)
            .map_err(AppError::Render)?;
        let zooms = warp::zoom_views(&file.zooms, start);
        let graph =
            warp::filter_graph(&spans, overlays.is_some(), &zooms, FPS).map_err(AppError::Warp)?;
        let args = warp::render_args(&zeroed, overlays.as_deref(), &graph, out);
        recorder::encode("ffmpeg", &args, out).map_err(AppError::Render)
    })();
    // Best effort; the render's own result is the one reported.
    let _ = fs::remove_dir_all(&scratch);
    rendered
}

/// Draws each frame of the overlays into `scratch`, once per distinct look, and returns the ffconcat list
/// joining them into one stream.
fn draw_overlays(frames: &[scene::Frame], scratch: &Path) -> Result<PathBuf, AppError> {
    let mut drawn: HashMap<String, String> = HashMap::new();
    let mut names = Vec::with_capacity(frames.len());
    for frame in frames {
        let key = format!("{:?}", frame.shown);
        if let Some(name) = drawn.get(&key) {
            names.push(name.clone());
            continue;
        }
        let name = format!("overlay{}.png", drawn.len());
        overlay::write_png(&overlay::frame(&frame.shown)?, &scratch.join(&name))?;
        drawn.insert(key, name.clone());
        names.push(name);
    }
    overlay::write_png(&overlay::gap(), &scratch.join("gap.png"))?;
    let spans: Vec<(Duration, Duration)> = frames.iter().map(|f| (f.from, f.until)).collect();
    let list = scratch.join("overlay.ffconcat");
    fs::write(&list, warp::overlay_list(&spans, &names, "gap.png")).map_err(|e| {
        AppError::Overlay(overlay::Error::Write(format!("{}: {e}", list.display())))
    })?;
    Ok(list)
}

impl From<overlay::Error> for AppError {
    fn from(error: overlay::Error) -> Self {
        Self::Overlay(error)
    }
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

    #[test]
    fn the_timeline_is_saved_without_drawing_anything() {
        let dir = std::env::temp_dir().join(format!("taperec-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let marks = warp::Marks {
            texts: vec![warp::MarkedText {
                start: Duration::from_secs(1),
                end: Duration::from_secs(2),
                text: "Part 1".into(),
            }],
            ..warp::Marks::default()
        };
        let path = dir.join("t.timeline.yaml");
        save_timeline(&path, "t.raw.mkv", marks).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("text: Part 1"), "{text}");
        assert!(!text.contains(".png"), "{text}");
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
                &tape("  - text: Building\n  - zoom: { point: { x: 400, y: 300 }, scale: 2 }\n  - sleep: 2s\n  - warp: { label: Build, length: 5s, steps: [sleep: 10s] }\n  - zoom: off\n  - sleep: 1s\n"),
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
            // The text shows from the start until the capture ends after the last 1 s.
            assert_eq!(
                marks.texts,
                [warp::MarkedText {
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
        fn a_tape_without_after_effects_marks_start_end_and_its_shortcuts() {
            let platform = FakePlatform::default();
            let mut recorder = platform.recorder();
            let mut desktop = Desktop::new(platform);
            let marked = capture(
                &tape("  - paste: echo hi\n  - sleep: 2s\n  - key: primary+k\n"),
                &mut desktop,
                &mut recorder,
                Path::new("/out/t.raw.mov"),
                None,
            )
            .unwrap();
            let names: Vec<&str> = marked.moments.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, ["start", "end"]);
            // Each shortcut at its step's start, labelled by the platform.
            let keys: Vec<(Duration, Vec<String>)> = marked
                .keys
                .iter()
                .map(|k| (k.at, k.labels.clone()))
                .collect();
            assert_eq!(
                keys,
                [
                    (Duration::ZERO, vec!["paste".to_owned()]),
                    (
                        Duration::from_secs(2),
                        vec!["primary".to_owned(), "k".to_owned()]
                    ),
                ]
            );
            assert_eq!(
                warp::Marks {
                    moments: Vec::new(),
                    keys: Vec::new(),
                    ..marked
                },
                warp::Marks::default()
            );
            assert_eq!(recorder.events.last(), Some(&Capture::Stopped));
        }

        #[test]
        fn marks_fall_on_the_capture_clock_with_the_position_found() {
            let platform = FakePlatform::default();
            let mut recorder = platform.recorder();
            let mut desktop = Desktop::new(platform);
            let marks = capture(
                &tape("  - sleep: 2s\n  - mark: typed\n  - sleep: 1s\n  - mark: { name: results, at: { point: { x: 400, y: 300 } } }\n  - sleep: 1s\n"),
                &mut desktop,
                &mut recorder,
                Path::new("/out/t.raw.mov"),
                None,
            )
            .unwrap();
            let moment = |name: &str, at, position| warp::Moment {
                name: name.into(),
                at: Duration::from_secs(at),
                position,
            };
            assert_eq!(
                marks.moments,
                [
                    moment("start", 0, None),
                    moment("typed", 2, None),
                    moment("results", 3, Some(tape::Point { x: 400, y: 300 })),
                    moment("end", 4, None),
                ]
            );
        }
    }
}
