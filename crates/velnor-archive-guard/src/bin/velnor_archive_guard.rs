//! Run the trusted bounded tar preflight on standard input.

use std::env;
use std::io;
use std::process::ExitCode;

use velnor_archive_guard::owned_archive_guard;

const FINGERPRINT: &str = env!("VELNOR_ARCHIVE_GUARD_FINGERPRINT");

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("archive guard: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let mode = arguments
        .next()
        .ok_or_else(|| "archive guard mode is required".to_owned())?
        .into_string()
        .map_err(|_| "archive guard mode is not UTF-8".to_owned())?;
    if mode == "--fingerprint" {
        if arguments.next().is_some() {
            return Err("unexpected archive guard argument".to_owned());
        }
        println!("{FINGERPRINT}");
        return Ok(());
    }
    if arguments.next().is_some() {
        return Err("unexpected archive guard argument".to_owned());
    }
    owned_archive_guard::validate(io::stdin().lock(), &mode)
}
