//! Command-line entry point for `taperec`.

use std::env;
use std::process::ExitCode;

use taperec::{app, cli};

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
