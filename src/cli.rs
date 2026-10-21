//! Command-line arguments.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// Usage text shown for `--help` and after argument errors.
pub const USAGE: &str = "usage: taperec <tape.yaml> [--out-dir DIR] [--failshot PNG] [--recorder auto|ffmpeg|screencapturekit]
       taperec check <tape.yaml>...
       taperec render <name.timeline.yaml> [--out FILE]
       taperec schema";

/// Parsed arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The tape to play.
    pub tape: PathBuf,
    /// Where the recording is written; defaults to the current directory.
    pub out_dir: PathBuf,
    /// Where to save a screenshot of the desktop if a step fails.
    pub failshot: Option<String>,
    /// How to capture the screen.
    pub recorder: RecorderChoice,
}

/// Which recorder captures the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecorderChoice {
    /// `ScreenCaptureKit` where the Mac encodes H.264 in hardware, ffmpeg elsewhere.
    #[default]
    Auto,
    /// ffmpeg's avfoundation input with libx264.
    Ffmpeg,
    /// `ScreenCaptureKit` with `VideoToolbox` in the desktop helper.
    ScreenCaptureKit,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Record a tape.
    Record(Args),
    /// Validate tapes without recording.
    Check(Vec<PathBuf>),
    /// Print the tape JSON Schema.
    Schema,
    /// Build a video from a recording's timeline file.
    Render {
        /// The timeline file.
        timeline: PathBuf,
        /// The video to write.
        out: PathBuf,
    },
    /// Print usage.
    Help,
    /// Print the version.
    Version,
}

/// Why the arguments were rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// A missing tape.
    MissingTape,
    /// A missing timeline file.
    MissingTimeline,
    /// A second tape.
    ExtraArgument(OsString),
    /// An unknown option.
    UnknownOption(OsString),
    /// An option whose value must be UTF-8 text.
    NotUtf8(&'static str),
    /// An option missing its value.
    MissingValue(&'static str),
    /// A `--recorder` other than `auto`, `ffmpeg` or `screencapturekit`.
    UnknownRecorder(OsString),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTape => write!(f, "no tape given"),
            Self::MissingTimeline => write!(f, "no timeline given"),
            Self::ExtraArgument(arg) => write!(f, "unexpected argument {}", arg.to_string_lossy()),
            Self::UnknownOption(arg) => write!(f, "unknown option {}", arg.to_string_lossy()),
            Self::NotUtf8(option) => write!(f, "{option} needs a UTF-8 path"),
            Self::MissingValue(option) => write!(f, "{option} needs a value"),
            Self::UnknownRecorder(name) => write!(
                f,
                "--recorder takes auto, ffmpeg or screencapturekit, not {}",
                name.to_string_lossy()
            ),
        }
    }
}

impl std::error::Error for UsageError {}

/// Parses arguments after the program name.
///
/// # Errors
///
/// A [`UsageError`] for anything beyond one tape, `--out-dir DIR` and `--failshot PNG`.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Invocation, UsageError> {
    let mut args = args.into_iter().peekable();
    match args.peek().and_then(|arg| arg.to_str()) {
        Some("check") => {
            args.next();
            let tapes: Vec<PathBuf> = args.map(PathBuf::from).collect();
            return if tapes.is_empty() {
                Err(UsageError::MissingTape)
            } else {
                Ok(Invocation::Check(tapes))
            };
        }
        Some("render") => {
            args.next();
            return render(args);
        }
        Some("schema") => {
            args.next();
            return match args.next() {
                Some(extra) => Err(UsageError::ExtraArgument(extra)),
                None => Ok(Invocation::Schema),
            };
        }
        _ => {}
    }
    let mut tape = None;
    let mut out_dir = None;
    let mut failshot = None;
    let mut recorder = RecorderChoice::Auto;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Invocation::Help),
            Some("-V" | "--version") => return Ok(Invocation::Version),
            Some("--failshot") => {
                let value = args.next().ok_or(UsageError::MissingValue("--failshot"))?;
                failshot = Some(
                    value
                        .into_string()
                        .map_err(|_| UsageError::NotUtf8("--failshot"))?,
                );
            }
            Some("--out-dir") => {
                out_dir = Some(args.next().ok_or(UsageError::MissingValue("--out-dir"))?);
            }
            Some("--recorder") => {
                let value = args.next().ok_or(UsageError::MissingValue("--recorder"))?;
                recorder = match value.to_str() {
                    Some("auto") => RecorderChoice::Auto,
                    Some("ffmpeg") => RecorderChoice::Ffmpeg,
                    Some("screencapturekit") => RecorderChoice::ScreenCaptureKit,
                    _ => return Err(UsageError::UnknownRecorder(value)),
                };
            }
            Some(option) if option.starts_with('-') => return Err(UsageError::UnknownOption(arg)),
            _ if tape.is_some() => return Err(UsageError::ExtraArgument(arg)),
            _ => tape = Some(arg),
        }
    }
    Ok(Invocation::Record(Args {
        tape: tape.ok_or(UsageError::MissingTape)?.into(),
        out_dir: out_dir.unwrap_or_else(|| ".".into()).into(),
        failshot,
        recorder,
    }))
}

/// Parses `render <timeline> [--out FILE]`; without `--out` the video sits next to the timeline.
fn render(args: impl Iterator<Item = OsString>) -> Result<Invocation, UsageError> {
    let mut args = args;
    let mut timeline: Option<PathBuf> = None;
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => {
                out = Some(args.next().ok_or(UsageError::MissingValue("--out"))?.into());
            }
            Some(option) if option.starts_with('-') => return Err(UsageError::UnknownOption(arg)),
            _ if timeline.is_some() => return Err(UsageError::ExtraArgument(arg)),
            _ => timeline = Some(arg.into()),
        }
    }
    let timeline = timeline.ok_or(UsageError::MissingTimeline)?;
    let out = if let Some(out) = out {
        out
    } else {
        let name = timeline
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let stem = name
            .strip_suffix(".timeline.yaml")
            .ok_or(UsageError::MissingValue("--out"))?;
        timeline.with_file_name(format!("{stem}.mp4"))
    };
    Ok(Invocation::Render { timeline, out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Invocation, UsageError> {
        parse(args.iter().map(OsString::from))
    }

    fn record(tape: &str, out_dir: &str) -> Invocation {
        Invocation::Record(Args {
            tape: tape.into(),
            out_dir: out_dir.into(),
            failshot: None,
            recorder: RecorderChoice::Auto,
        })
    }

    #[test]
    fn recorder_is_auto_unless_chosen() {
        let chosen = |name: &str| match parse_strs(&["demo.yaml", "--recorder", name]) {
            Ok(Invocation::Record(args)) => Ok(args.recorder),
            other => Err(other),
        };
        assert_eq!(chosen("ffmpeg"), Ok(RecorderChoice::Ffmpeg));
        assert_eq!(
            chosen("screencapturekit"),
            Ok(RecorderChoice::ScreenCaptureKit)
        );
        assert_eq!(chosen("auto"), Ok(RecorderChoice::Auto));
        assert_eq!(
            parse_strs(&["demo.yaml", "--recorder", "obs"]),
            Err(UsageError::UnknownRecorder("obs".into()))
        );
        assert_eq!(
            parse_strs(&["demo.yaml", "--recorder"]),
            Err(UsageError::MissingValue("--recorder"))
        );
    }

    #[test]
    fn failshot_takes_a_path() {
        assert_eq!(
            parse_strs(&["demo.yaml", "--failshot", "/out/fail.png"]),
            Ok(Invocation::Record(Args {
                tape: "demo.yaml".into(),
                out_dir: ".".into(),
                failshot: Some("/out/fail.png".into()),
                recorder: RecorderChoice::Auto,
            }))
        );
        assert_eq!(
            parse_strs(&["demo.yaml", "--failshot"]),
            Err(UsageError::MissingValue("--failshot"))
        );
    }

    #[test]
    fn failshot_needs_a_utf8_path() {
        use std::os::unix::ffi::OsStringExt;
        let args = [
            OsString::from("demo.yaml"),
            OsString::from("--failshot"),
            OsString::from_vec(b"/out/\xff.png".to_vec()),
        ];
        assert_eq!(parse(args), Err(UsageError::NotUtf8("--failshot")));
    }

    #[test]
    fn tape_alone_records_into_current_directory() {
        assert_eq!(parse_strs(&["demo.yaml"]), Ok(record("demo.yaml", ".")));
    }

    #[test]
    fn out_dir_before_or_after_tape() {
        assert_eq!(
            parse_strs(&["demo.yaml", "--out-dir", "/out"]),
            Ok(record("demo.yaml", "/out"))
        );
        assert_eq!(
            parse_strs(&["--out-dir", "/out", "demo.yaml"]),
            Ok(record("demo.yaml", "/out"))
        );
    }

    #[test]
    fn version_wins() {
        assert_eq!(parse_strs(&["--version"]), Ok(Invocation::Version));
        assert_eq!(parse_strs(&["demo.yaml", "-V"]), Ok(Invocation::Version));
    }

    #[test]
    fn help_wins() {
        assert_eq!(parse_strs(&["--help"]), Ok(Invocation::Help));
        assert_eq!(parse_strs(&["demo.yaml", "-h"]), Ok(Invocation::Help));
    }

    #[test]
    fn check_takes_one_or_more_tapes() {
        assert_eq!(
            parse_strs(&["check", "a.yaml", "b.yaml"]),
            Ok(Invocation::Check(vec!["a.yaml".into(), "b.yaml".into()]))
        );
        assert_eq!(parse_strs(&["check"]), Err(UsageError::MissingTape));
    }

    #[test]
    fn render_writes_next_to_the_timeline_unless_told() {
        assert_eq!(
            parse_strs(&["render", "out/smoke.timeline.yaml"]),
            Ok(Invocation::Render {
                timeline: "out/smoke.timeline.yaml".into(),
                out: "out/smoke.mp4".into(),
            })
        );
        assert_eq!(
            parse_strs(&["render", "t.yaml", "--out", "cut.mp4"]),
            Ok(Invocation::Render {
                timeline: "t.yaml".into(),
                out: "cut.mp4".into(),
            })
        );
        assert_eq!(
            parse_strs(&["render", "t.yaml"]),
            Err(UsageError::MissingValue("--out"))
        );
        assert_eq!(parse_strs(&["render"]), Err(UsageError::MissingTimeline));
    }

    #[test]
    fn schema_takes_nothing() {
        assert_eq!(parse_strs(&["schema"]), Ok(Invocation::Schema));
        assert_eq!(
            parse_strs(&["schema", "x"]),
            Err(UsageError::ExtraArgument("x".into()))
        );
    }

    #[test]
    fn rejects_bad_arguments() {
        assert_eq!(parse_strs(&[]), Err(UsageError::MissingTape));
        assert_eq!(
            parse_strs(&["a.yaml", "b.yaml"]),
            Err(UsageError::ExtraArgument("b.yaml".into()))
        );
        assert_eq!(
            parse_strs(&["--fast", "a.yaml"]),
            Err(UsageError::UnknownOption("--fast".into()))
        );
        assert_eq!(
            parse_strs(&["a.yaml", "--out-dir"]),
            Err(UsageError::MissingValue("--out-dir"))
        );
    }
}
