// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod binary_format;
mod capture;
mod filesystem;
mod guard;

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use capture::{Mode, run};

const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 120_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 3_600_000;
const DEFAULT_OUTPUT_CAP_BYTES: usize = 8 * 1024 * 1024;
const MAX_OUTPUT_CAP_BYTES: usize = 64 * 1024 * 1024;
const COMMAND_CLEANUP_BUDGET: Duration = Duration::from_secs(2);

pub fn run_cli() -> u8 {
    match parse_args(env::args_os().skip(1)).and_then(run) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("velnor-golden-collector: {error}");
            error.exit_code()
        }
    }
}

#[derive(Debug)]
struct Arguments {
    mode: Mode,
    root: PathBuf,
    cli: PathBuf,
    guard: PathBuf,
    guard_sha256: String,
    command_timeout: Duration,
    output_cap_bytes: usize,
}

fn parse_args(values: impl Iterator<Item = OsString>) -> Result<Arguments, CollectorError> {
    let mut values = values.map(|value| {
        value
            .into_string()
            .map_err(|_| CollectorError::usage("arguments must be valid UTF-8"))
    });
    let mode = match values.next().transpose()?.as_deref() {
        Some("capture") => Mode::Capture,
        Some("check") => Mode::Check,
        _ => return Err(CollectorError::usage(usage())),
    };
    let mut root = None;
    let mut cli = None;
    let mut guard = None;
    let mut guard_sha256 = None;
    let mut command_timeout_ms = None;
    let mut output_cap_bytes = None;
    while let Some(key) = values.next() {
        let key = key?;
        let value = values
            .next()
            .transpose()?
            .ok_or_else(|| CollectorError::usage(format!("missing value for {key}")))?;
        match key.as_str() {
            "--root" => set_once(&mut root, value, &key)?,
            "--cli" => set_once(&mut cli, value, &key)?,
            "--archive-guard" => set_once(&mut guard, value, &key)?,
            "--archive-guard-sha256" => set_once(&mut guard_sha256, value, &key)?,
            "--command-timeout-ms" => set_once(
                &mut command_timeout_ms,
                parse_bounded_integer(&value, &key, MAX_COMMAND_TIMEOUT_MS)?,
                &key,
            )?,
            "--output-cap-bytes" => {
                let cap = parse_bounded_integer(
                    &value,
                    &key,
                    u64::try_from(MAX_OUTPUT_CAP_BYTES).unwrap_or(u64::MAX),
                )?;
                set_once(
                    &mut output_cap_bytes,
                    usize::try_from(cap)
                        .map_err(|_| CollectorError::usage(format!("{key} is too large")))?,
                    &key,
                )?;
            }
            _ => return Err(CollectorError::usage(usage())),
        }
    }
    let caller = env::current_dir().map_err(CollectorError::operational)?;
    Ok(Arguments {
        mode,
        root: absolute(root, "--root", &caller)?,
        cli: absolute(cli, "--cli", &caller)?,
        guard: guard
            .ok_or_else(|| CollectorError::usage("--archive-guard is required"))?
            .into(),
        guard_sha256: guard_sha256
            .ok_or_else(|| CollectorError::usage("--archive-guard-sha256 is required"))?,
        command_timeout: Duration::from_millis(
            command_timeout_ms.unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS),
        ),
        output_cap_bytes: output_cap_bytes.unwrap_or(DEFAULT_OUTPUT_CAP_BYTES),
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), CollectorError> {
    if slot.replace(value).is_some() {
        Err(CollectorError::usage(format!("{name} must appear once")))
    } else {
        Ok(())
    }
}

fn parse_bounded_integer(value: &str, name: &str, max: u64) -> Result<u64, CollectorError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| CollectorError::usage(format!("{name} must be a positive integer")))?;
    if !(1..=max).contains(&parsed) {
        return Err(CollectorError::usage(format!(
            "{name} must be between 1 and {max}"
        )));
    }
    Ok(parsed)
}

fn absolute(
    value: Option<String>,
    name: &str,
    caller: &std::path::Path,
) -> Result<PathBuf, CollectorError> {
    let value = value.ok_or_else(|| CollectorError::usage(format!("{name} is required")))?;
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        caller.join(path)
    })
}

fn usage() -> String {
    "usage: velnor-golden-collector capture|check --root PATH --cli PATH --archive-guard PATH --archive-guard-sha256 SHA256 [--command-timeout-ms MS] [--output-cap-bytes BYTES]".into()
}

#[derive(Debug)]
pub(crate) struct CollectorError {
    message: String,
    exit_code: u8,
}

impl CollectorError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
        }
    }

    fn operational(message: impl IntoErrorMessage) -> Self {
        Self {
            message: message.into_error_message(),
            exit_code: 2,
        }
    }

    pub(crate) fn mismatch(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
        }
    }

    pub(crate) fn exit_code(&self) -> u8 {
        self.exit_code
    }
}

trait IntoErrorMessage {
    fn into_error_message(self) -> String;
}

impl<T: std::fmt::Display> IntoErrorMessage for T {
    fn into_error_message(self) -> String {
        self.to_string()
    }
}

impl std::fmt::Display for CollectorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CollectorError {}
