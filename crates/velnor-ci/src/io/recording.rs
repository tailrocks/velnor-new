// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::telemetry::{INVOCATIONS, Invocation, configuration};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

fn temp(file: &str) -> std::path::PathBuf {
    Path::new(&std::env::var("RUNNER_TEMP").unwrap_or_default()).join(file)
}

/// The Cargo command that `velnor-ci inputs build [--directory <dir>]... -- <command>` wraps, or
/// `argv` itself. The wrapper only inventories inputs around the command, so the report, the
/// cache audit and the Cargo root come from the command it runs.
pub(super) fn cargo_command(argv: &[String]) -> &[String] {
    let wrapper = argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        .is_some_and(|name| name == "velnor-ci")
        && argv.get(1).is_some_and(|word| word == "inputs")
        && argv.get(2).is_some_and(|word| word == "build");
    let inner = argv
        .iter()
        .position(|word| word == "--")
        .and_then(|separator| argv.get(separator + 1..))
        .filter(|inner| !inner.is_empty());
    match inner {
        Some(inner) if wrapper => inner,
        _ => argv,
    }
}

pub(super) fn start_invocation(argv: &[String]) -> Result<Invocation, CiError> {
    let execution = execution(argv)?;
    Ok(Invocation {
        argv: argv.to_vec(),
        root: root(argv)?,
        configuration: format!(
            "{} {:?}",
            configuration(argv),
            execution
                .iter()
                .filter(|(key, _)| [
                    "RUSTUP_TOOLCHAIN",
                    "RUSTFLAGS",
                    "CARGO_ENCODED_RUSTFLAGS",
                    "CARGO_BUILD_TARGET"
                ]
                .contains(&key.as_str())
                    || key.starts_with("CARGO_PROFILE_"))
                .collect::<BTreeMap<_, _>>()
        ),
        started_at: u64::try_from(super::now_seconds()).unwrap_or(0),
        seconds: 0.0,
        log_start: 0,
        log_end: 0,
        compilations: Vec::new(),
        downloads: Vec::new(),
        verified_download_archives: None,
        verified_git_downloads: None,
        restored_target_usable: None,
        artifacts: Vec::new(),
        progress: Some(Vec::new()),
        capture_complete: false,
        tests: Vec::new(),
        rejected: 0,
        execution,
        target_directory: target_directory(argv)?,
    })
}
pub(super) fn save_invocation(invocation: &Invocation) -> Result<(), CiError> {
    writeln!(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(temp(INVOCATIONS))
            .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?,
        "{}",
        serde_json::to_string(invocation)
            .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?
    )
    .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))
}

fn root(argv: &[String]) -> Result<String, CiError> {
    let directory = super::current_dir()?;
    let Some(manifest) = manifest_path(argv) else {
        return Ok(directory.to_string_lossy().into_owned());
    };
    let path = directory.join(manifest);
    Ok(path
        .parent()
        .unwrap_or(&directory)
        .to_string_lossy()
        .into_owned())
}

fn manifest_path(argv: &[String]) -> Option<&str> {
    let mut words = argv.iter().skip(1).take_while(|word| word.as_str() != "--");
    while let Some(word) = words.next() {
        if let Some(value) = word.strip_prefix("--manifest-path=") {
            return Some(value);
        }
        if word == "--manifest-path" {
            return words.next().map(String::as_str);
        }
    }
    None
}

fn target_directory(argv: &[String]) -> Result<Option<String>, CiError> {
    let mut target = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from);
    let mut words = argv.iter().take_while(|word| word.as_str() != "--");
    while let Some(word) = words.next() {
        let value = word.strip_prefix("--target-dir=").or_else(|| {
            (word == "--target-dir")
                .then(|| words.next().map(String::as_str))
                .flatten()
        });
        if let Some(value) = value {
            target = Some(value.into());
        }
    }
    target
        .map(|path| super::current_dir().map(|cwd| cwd.join(path).to_string_lossy().into_owned()))
        .transpose()
}

fn execution(argv: &[String]) -> Result<BTreeMap<String, String>, CiError> {
    let mut settings: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_str()?;
            let recognized = matches!(
                key,
                "RUSTUP_TOOLCHAIN"
                    | "RUSTFLAGS"
                    | "RUSTDOCFLAGS"
                    | "RUSTC"
                    | "RUSTDOC"
                    | "RUSTC_WRAPPER"
                    | "RUSTC_WORKSPACE_WRAPPER"
                    | "RUSTC_BOOTSTRAP"
                    | "CARGO_BUILD_RUSTC"
                    | "CARGO_BUILD_RUSTC_WRAPPER"
                    | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
            ) || key.starts_with("CARGO_PROFILE_")
                || (key.starts_with("CARGO_TARGET_") && key != "CARGO_TARGET_DIR")
                || matches!(key, "CARGO_BUILD_TARGET" | "CARGO_ENCODED_RUSTFLAGS");
            if !recognized {
                return None;
            }
            Some(
                value
                    .into_string()
                    .map(|value| (key.to_owned(), value))
                    .map_err(|_| {
                        CiError::new(format!("compiler setting {key} is not valid Unicode"))
                    }),
            )
        })
        .collect::<Result<_, _>>()?;
    settings.insert(
        "host-platform".to_owned(),
        format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
    );
    let mut words = argv.iter().take_while(|word| word.as_str() != "--");
    let mut configs = Vec::new();
    if let Some(toolchain) = argv.get(1).filter(|word| word.starts_with('+')) {
        settings.insert("toolchain-argument".to_owned(), toolchain.clone());
    }
    while let Some(word) = words.next() {
        if let Some(config) = word.strip_prefix("--config=") {
            configs.push(config.to_owned());
        } else if word == "--config" {
            configs.extend(words.next().cloned());
        }
    }
    settings.insert("cargo-config-arguments".to_owned(), format!("{configs:?}"));
    Ok(settings)
}
