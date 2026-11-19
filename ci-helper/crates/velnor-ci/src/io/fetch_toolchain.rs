// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::error::CiError;

const PROXY: &str = "rustup";
const TOOLCHAIN_VARIABLE: &str = "RUSTUP_TOOLCHAIN";
const PASSED: [&str; 3] = ["PATH", "HOME", "RUSTUP_HOME"];

pub(super) fn pin(
    cargo: &Path,
    environment: &[(OsString, OsString)],
) -> Result<Option<OsString>, CiError> {
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
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_dir())
        .ok_or_else(|| CiError::new("CARGO_HOME is not a directory"))?;
    let mut command = Command::new(&resolved);
    command
        .current_dir(neutral)
        .env_clear()
        .args(["show", "active-toolchain"]);
    for name in PASSED {
        if let Some(value) = value(name) {
            command.env(name, value);
        }
    }
    if let Some(chosen) = std::env::var(TOOLCHAIN_VARIABLE)
        .ok()
        .filter(|chosen| named(chosen))
    {
        command.env(TOOLCHAIN_VARIABLE, chosen);
    }
    let output = command
        .output()
        .map_err(|e| CiError::new(format!("cannot run {}: {e}", resolved.display())))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let name = text.split_whitespace().next().unwrap_or_default();
    if !output.status.success() || !named(name) {
        return Err(CiError::new(format!(
            "rustup did not name an installed toolchain ({name:?}); refusing to run cargo with the token"
        )));
    }
    Ok(Some(name.into()))
}

fn named(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
