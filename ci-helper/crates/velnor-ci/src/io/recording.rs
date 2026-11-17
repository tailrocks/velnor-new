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

pub(super) fn start_invocation(argv: &[String]) -> Result<Invocation, CiError> {
    let execution = execution(argv);
    Ok(Invocation {
        argv: argv.to_vec(),
        root: super::current_dir()?.to_string_lossy().into_owned(),
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

fn execution(argv: &[String]) -> BTreeMap<String, String> {
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
            Some((key.to_owned(), value.into_string().ok()?))
        })
        .collect();
    settings.insert(
        "host-platform".to_owned(),
        format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
    );
    let mut words = argv.iter().take_while(|word| word.as_str() != "--");
    let mut configs = Vec::new();
    while let Some(word) = words.next() {
        if let Some(config) = word.strip_prefix("--config=") {
            configs.push(config.to_owned());
        } else if word == "--config" {
            configs.extend(words.next().cloned());
        } else if word.starts_with('+') {
            settings.insert("toolchain-argument".to_owned(), word.clone());
        }
    }
    settings.insert("cargo-config-arguments".to_owned(), format!("{configs:?}"));
    settings
}
