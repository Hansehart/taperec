//! Command-line entry point for `taperec`.

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use taperec::{app, cli, tape};

fn main() -> ExitCode {
    match cli::parse(env::args_os().skip(1)) {
        Ok(cli::Invocation::Version) => {
            eprintln!("taperec {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(cli::Invocation::Help) => {
            eprintln!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(cli::Invocation::Schema) => match io::stdout().write_all(tape::schema().as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("taperec: cannot write the schema: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(cli::Invocation::Render { timeline, out }) => match app::render(&timeline, &out) {
            Ok(()) => {
                eprintln!("taperec: wrote {}", out.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("taperec: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(cli::Invocation::Check(tapes)) => {
            let failures = app::check(&tapes);
            for failure in &failures {
                eprintln!("taperec: {failure}");
            }
            if failures.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Ok(cli::Invocation::Record(args)) => match app::record(&args, env::consts::OS) {
            Ok(path) => {
                eprintln!("taperec: wrote {}", path.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("taperec: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("taperec: {error}\n{}", cli::USAGE);
            ExitCode::from(2)
        }
    }
}
