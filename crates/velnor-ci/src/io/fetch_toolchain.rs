// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CiError;

use super::fetch::Trusted;

const PROXY: &str = "rustup";
const TOOLCHAIN_VARIABLE: &str = "RUSTUP_TOOLCHAIN";
const PASSED: &[&str] = &["PATH", "HOME", "RUSTUP_HOME"];

pub(super) struct Pinned {
    pub(super) name: OsString,
    /// The toolchain's own cargo from `rustup which cargo`; `None` leaves the proxy to run it.
    pub(super) cargo: Option<PathBuf>,
}

pub(super) fn pin(
    cargo: &Path,
    environment: &[(OsString, OsString)],
    trusted: &Trusted,
) -> Result<Option<Pinned>, CiError> {
    let Ok(resolved) = std::fs::canonicalize(cargo) else {
        return Ok(None);
    };
    if resolved.file_name().and_then(|name| name.to_str()) != Some(PROXY) {
        return Ok(None);
    }
    let value = |name: &str| {
        environment
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let neutral = value("CARGO_HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_dir())
        .ok_or_else(|| CiError::new("CARGO_HOME is not a directory"))?;
    let rustup = |arguments: [&str; 2], toolchain: Option<&str>| {
        let mut command = Command::new(&resolved);
        command.current_dir(&neutral).env_clear().args(arguments);
        for name in PASSED.iter().copied() {
            if let Some(value) = value(name) {
                command.env(name, value);
            }
        }
        if let Some(chosen) = toolchain {
            command.env(TOOLCHAIN_VARIABLE, chosen);
        }
        command
            .output()
            .map_err(|e| CiError::new(format!("cannot run {}: {e}", resolved.display())))
    };
    let chosen = std::env::var(TOOLCHAIN_VARIABLE)
        .ok()
        .filter(|chosen| named(chosen));
    let output = rustup(["show", "active-toolchain"], chosen.as_deref())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let name = text.split_whitespace().next().unwrap_or_default();
    if !output.status.success() || !named(name) {
        return Err(CiError::new(format!(
            "rustup did not name an installed toolchain ({name:?}); refusing to run cargo with the token"
        )));
    }
    let which = rustup(["which", "cargo"], Some(name))?;
    let cargo = which
        .status
        .success()
        .then(|| real_cargo(&String::from_utf8_lossy(&which.stdout), trusted))
        .flatten();
    Ok(Some(Pinned {
        name: name.into(),
        cargo,
    }))
}

/// The toolchain cargo `rustup which` printed, when it is a real file the runner installed.
fn real_cargo(printed: &str, trusted: &Trusted) -> Option<PathBuf> {
    let path = std::fs::canonicalize(printed.lines().next()?.trim()).ok()?;
    (path.is_file()
        && path.file_name().is_some_and(|name| name == "cargo")
        && trusted.within(&path)
        && !super::fetch_real_cargo::is_mise_front(&path, &path))
    .then_some(path)
}

fn named(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
