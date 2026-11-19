// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
use std::env;
use std::ffi::OsString;
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
    if let Some(result) = qualification_mode(&mode, &mut arguments) {
        return result;
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

fn qualification_mode(
    mode: &str,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Option<Result<(), String>> {
    let result = match mode {
        "qualification-manifest" => {
            let Some(path) = arguments.next() else {
                return Some(Err("candidate manifest path is required".into()));
            };
            if arguments.next().is_some() {
                return Some(Err("unexpected archive guard argument".into()));
            }
            owned_archive_guard::validate_qualification_manifest(std::path::Path::new(&path))
        }
        "qualification-output-bindings" => {
            let Some(manifest) = arguments.next() else {
                return Some(Err("candidate manifest path is required".into()));
            };
            let Some(root) = arguments.next() else {
                return Some(Err("generated workflow root is required".into()));
            };
            if arguments.next().is_some() {
                return Some(Err("unexpected archive guard argument".into()));
            }
            owned_archive_guard::validate_qualification_output_bindings(
                std::path::Path::new(&manifest),
                std::path::Path::new(&root),
            )
        }
        _ => return None,
    };
    Some(result)
}
