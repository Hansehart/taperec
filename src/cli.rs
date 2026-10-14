//! Command-line arguments.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

/// Usage text shown for `--help` and after argument errors.
pub const USAGE: &str = "usage: taperec <file.tape> [--out-dir DIR] [--failshot PNG]";

/// Parsed arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The tape to play.
    pub tape: PathBuf,
    /// Where the recording is written; defaults to the current directory.
    pub out_dir: PathBuf,
    /// Where to save a screenshot of the desktop if a step fails.
    pub failshot: Option<String>,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Record a tape.
    Record(Args),
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
    /// A second tape.
    ExtraArgument(OsString),
    /// An unknown option.
    UnknownOption(OsString),
    /// An option whose value must be UTF-8 text.
    NotUtf8(&'static str),
    /// An option missing its value.
    MissingValue(&'static str),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTape => write!(f, "no tape given"),
            Self::ExtraArgument(arg) => write!(f, "unexpected argument {}", arg.to_string_lossy()),
            Self::UnknownOption(arg) => write!(f, "unknown option {}", arg.to_string_lossy()),
            Self::NotUtf8(option) => write!(f, "{option} needs a UTF-8 path"),
            Self::MissingValue(option) => write!(f, "{option} needs a value"),
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
    let mut args = args.into_iter();
    let mut tape = None;
    let mut out_dir = None;
    let mut failshot = None;
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
            Some(option) if option.starts_with('-') => return Err(UsageError::UnknownOption(arg)),
            _ if tape.is_some() => return Err(UsageError::ExtraArgument(arg)),
            _ => tape = Some(arg),
        }
    }
    Ok(Invocation::Record(Args {
        tape: tape.ok_or(UsageError::MissingTape)?.into(),
        out_dir: out_dir.unwrap_or_else(|| ".".into()).into(),
        failshot,
    }))
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
        })
    }

    #[test]
    fn failshot_takes_a_path() {
        assert_eq!(
            parse_strs(&["demo.tape", "--failshot", "/out/fail.png"]),
            Ok(Invocation::Record(Args {
                tape: "demo.tape".into(),
                out_dir: ".".into(),
                failshot: Some("/out/fail.png".into()),
            }))
        );
        assert_eq!(
            parse_strs(&["demo.tape", "--failshot"]),
            Err(UsageError::MissingValue("--failshot"))
        );
    }

    #[test]
    fn failshot_needs_a_utf8_path() {
        use std::os::unix::ffi::OsStringExt;
        let args = [
            OsString::from("demo.tape"),
            OsString::from("--failshot"),
            OsString::from_vec(b"/out/\xff.png".to_vec()),
        ];
        assert_eq!(parse(args), Err(UsageError::NotUtf8("--failshot")));
    }

    #[test]
    fn tape_alone_records_into_current_directory() {
        assert_eq!(parse_strs(&["demo.tape"]), Ok(record("demo.tape", ".")));
    }

    #[test]
    fn out_dir_before_or_after_tape() {
        assert_eq!(
            parse_strs(&["demo.tape", "--out-dir", "/out"]),
            Ok(record("demo.tape", "/out"))
        );
        assert_eq!(
            parse_strs(&["--out-dir", "/out", "demo.tape"]),
            Ok(record("demo.tape", "/out"))
        );
    }

    #[test]
    fn version_wins() {
        assert_eq!(parse_strs(&["--version"]), Ok(Invocation::Version));
        assert_eq!(parse_strs(&["demo.tape", "-V"]), Ok(Invocation::Version));
    }

    #[test]
    fn help_wins() {
        assert_eq!(parse_strs(&["--help"]), Ok(Invocation::Help));
        assert_eq!(parse_strs(&["demo.tape", "-h"]), Ok(Invocation::Help));
    }

    #[test]
    fn rejects_bad_arguments() {
        assert_eq!(parse_strs(&[]), Err(UsageError::MissingTape));
        assert_eq!(
            parse_strs(&["a.tape", "b.tape"]),
            Err(UsageError::ExtraArgument("b.tape".into()))
        );
        assert_eq!(
            parse_strs(&["--fast", "a.tape"]),
            Err(UsageError::UnknownOption("--fast".into()))
        );
        assert_eq!(
            parse_strs(&["a.tape", "--out-dir"]),
            Err(UsageError::MissingValue("--out-dir"))
        );
    }
}
