//! End-to-end checks of the binary's exit codes and messages.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

const USAGE: &str = "usage: taperec <tape.yaml> [--out-dir DIR] [--failshot PNG]
       taperec check <tape.yaml>...
       taperec schema\n";

/// Runs the binary, returning its exit code and stderr.
#[expect(
    clippy::disallowed_methods,
    reason = "integration tests run the real binary"
)]
fn taperec(args: &[&str]) -> Result<(Option<i32>, String), Box<dyn Error>> {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(env!("CARGO_BIN_EXE_taperec"))
        .args(args)
        .output()?;
    // taperec reports on stderr and keeps stdout clean.
    assert_eq!(stdout, b"");
    Ok((status.code(), String::from_utf8(stderr)?))
}

/// Writes `content` to a tape file unique to this test run.
fn tape(name: &str, content: &str) -> Result<String, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("taperec-cli-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let path: PathBuf = dir.join(format!("{name}.yaml"));
    fs::write(&path, content)?;
    path.into_os_string()
        .into_string()
        .map_err(|p| format!("non-UTF-8 path {}", p.display()).into())
}

#[test]
fn help_prints_usage_and_succeeds() -> TestResult {
    assert_eq!(taperec(&["--help"])?, (Some(0), USAGE.to_owned()));
    Ok(())
}

#[test]
fn version_prints_name_and_version() -> TestResult {
    assert_eq!(
        taperec(&["--version"])?,
        (Some(0), format!("taperec {}\n", env!("CARGO_PKG_VERSION")))
    );
    Ok(())
}

#[test]
fn usage_errors_exit_2_with_usage() -> TestResult {
    assert_eq!(
        taperec(&[])?,
        (Some(2), format!("taperec: no tape given\n{USAGE}"))
    );
    assert_eq!(
        taperec(&["--fast", "a.yaml"])?,
        (Some(2), format!("taperec: unknown option --fast\n{USAGE}"))
    );
    Ok(())
}

#[test]
fn missing_tape_exits_1() -> TestResult {
    let (code, message) = taperec(&["/nonexistent/demo.yaml"])?;
    assert_eq!(code, Some(1));
    assert!(
        message.starts_with("taperec: cannot read /nonexistent/demo.yaml: "),
        "{message}"
    );
    Ok(())
}

#[test]
fn parse_errors_name_file_line_column_and_problem() -> TestResult {
    let path = tape("bad", "output: a.mp4\nsteps:\n  - key: Return\n")?;
    assert_eq!(
        taperec(&[&path])?,
        (
            Some(1),
            format!(
                "taperec: {path}:3:10: expected keys like `return` or `primary+shift+p`, got `Return`\n"
            )
        )
    );
    Ok(())
}

#[test]
fn rule_errors_name_the_step() -> TestResult {
    let path = tape("rule", "output: a.mp4\nsteps:\n  - set: {}\n")?;
    assert_eq!(
        taperec(&[&path])?,
        (
            Some(1),
            format!("taperec: {path}: steps[0]: `set` needs `typingSpeed` or `moveDuration`\n")
        )
    );
    Ok(())
}

#[test]
fn check_validates_without_recording() -> TestResult {
    let good = tape("checkgood", "output: a.mp4\nsteps:\n  - sleep: 1s\n")?;
    let bad = tape("checkbad", "output: a.mp4\nsteps:\n  - sleep: 5x\n")?;
    assert_eq!(taperec(&["check", &good])?, (Some(0), String::new()));
    let (code, message) = taperec(&["check", &good, &bad])?;
    assert_eq!(code, Some(1));
    assert!(
        message.starts_with(&format!("taperec: {bad}:3:")),
        "{message}"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "integration tests run the real binary"
)]
fn schema_prints_json_on_stdout() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_taperec"))
        .arg("schema")
        .output()?;
    assert_eq!(output.status.code(), Some(0));
    let schema = String::from_utf8(output.stdout)?;
    assert!(
        schema.starts_with('{') && schema.contains("\"steps\""),
        "{schema}"
    );
    Ok(())
}

#[test]
#[cfg(not(target_os = "macos"))]
fn records_only_on_macos() -> TestResult {
    let path = tape("ok", "output: a.mp4\nsteps:\n  - sleep: 1s\n")?;
    let (code, message) = taperec(&[&path])?;
    assert_eq!(code, Some(1));
    assert_eq!(
        message,
        format!(
            "taperec: taperec records the macOS desktop, not {}; render tapes with the render workflow on GitHub Actions\n",
            std::env::consts::OS
        )
    );
    Ok(())
}
