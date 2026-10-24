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
    if mode == "source-fingerprint" {
        let root = arguments
            .next()
            .ok_or_else(|| "archive checkout path is required".to_owned())?;
        if arguments.next().is_some() {
            return Err("unexpected archive guard argument".to_owned());
        }
        let root = std::path::PathBuf::from(root);
        let fingerprint = velnor_archive_guard::source_fingerprint(&root)?;
        if fingerprint.len() != 64
            || !fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("archive source fingerprint is malformed".to_owned());
        }
        println!("{fingerprint}");
        return Ok(());
    }
    if mode == "extract-candidate" {
        let archive = arguments
            .next()
            .ok_or_else(|| "candidate archive path is required".to_owned())?;
        let destination = arguments
            .next()
            .ok_or_else(|| "candidate output directory is required".to_owned())?;
        let executable = arguments
            .next()
            .ok_or_else(|| "candidate executable member is required".to_owned())?
            .into_string()
            .map_err(|_| "candidate executable member is not UTF-8".to_owned())?;
        let checksum = arguments
            .next()
            .ok_or_else(|| "candidate checksum member is required".to_owned())?
            .into_string()
            .map_err(|_| "candidate checksum member is not UTF-8".to_owned())?;
        let provenance = arguments
            .next()
            .ok_or_else(|| "candidate provenance member is required".to_owned())?
            .into_string()
            .map_err(|_| "candidate provenance member is not UTF-8".to_owned())?;
        if arguments.next().is_some() {
            return Err("unexpected archive guard argument".to_owned());
        }
        return velnor_archive_guard::owned_archive_guard::extract_candidate(
            std::path::Path::new(&archive),
            std::path::Path::new(&destination),
            [&executable, &checksum, &provenance],
        );
    }
    if arguments.next().is_some() {
        return Err("unexpected archive guard argument".to_owned());
    }
    owned_archive_guard::validate(io::stdin().lock(), &mode)
}
