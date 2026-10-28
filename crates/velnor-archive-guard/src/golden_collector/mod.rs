// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod binary_format;
mod capture;
mod filesystem;
mod guard;

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use capture::{Mode, run};

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
    while let Some(key) = values.next() {
        let key = key?;
        let value = values
            .next()
            .transpose()?
            .ok_or_else(|| CollectorError::usage(format!("missing value for {key}")))?;
        let slot = match key.as_str() {
            "--root" => &mut root,
            "--cli" => &mut cli,
            "--archive-guard" => &mut guard,
            "--archive-guard-sha256" => &mut guard_sha256,
            _ => return Err(CollectorError::usage(usage())),
        };
        if slot.replace(value).is_some() {
            return Err(CollectorError::usage(format!("{key} must appear once")));
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
    })
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
    "usage: velnor-golden-collector capture|check --root PATH --cli PATH --archive-guard PATH --archive-guard-sha256 SHA256".into()
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
