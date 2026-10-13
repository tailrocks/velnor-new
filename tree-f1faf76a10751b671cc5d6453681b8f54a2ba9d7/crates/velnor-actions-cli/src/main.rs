//! `velnor-actions` command-line entry point (Gate 0 shell).
//!
//! Owns argument parsing, typed dispatch, plan rendering, and exit codes.
//! Must not own orchestration algorithms or domain rules. The Clap parser
//! arrives in a later gate; this shell keeps a std-only `--version`/`--help`
//! surface so the binary target exists and is testable with zero dependencies.

use std::process::ExitCode;

/// Binary name; the only target name without the package-purpose suffix.
const BINARY_NAME: &str = "velnor-actions";

/// Shell usage text printed for `--help` and unknown arguments.
const USAGE: &str = "usage: velnor-actions [--version] [--help]\n";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("--help" | "-h") => {
            print!("{BINARY_NAME} {}\n{USAGE}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("{BINARY_NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(_) => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
