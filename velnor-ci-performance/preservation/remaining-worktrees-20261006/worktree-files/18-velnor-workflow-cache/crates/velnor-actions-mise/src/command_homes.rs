//! Controlled tool-home environment extension for Mise commands.

use std::ffi::OsStr;

use super::{
    CARGO_HOME_ENV, IsolatedCommand, MISE_CARGO_HOME_ENV, MISE_DATA_DIR_ENV, MISE_RUSTUP_HOME_ENV,
    RUSTUP_HOME_ENV, RUSTUP_TOOLCHAIN_ENV,
};
use crate::error::MiseError;

const TOOL_HOME_KEYS: [&str; 6] = [
    MISE_DATA_DIR_ENV,
    MISE_RUSTUP_HOME_ENV,
    MISE_CARGO_HOME_ENV,
    RUSTUP_HOME_ENV,
    CARGO_HOME_ENV,
    RUSTUP_TOOLCHAIN_ENV,
];

impl IsolatedCommand {
    /// Attach the exact typed Mise and Rust tool homes.
    ///
    /// This path admits `MISE_DATA_DIR` only from `ToolHomes`. General
    /// task env input keeps that key reserved.
    /// # Errors
    pub(crate) fn with_toolchain_env(
        mut self,
        toolchain_env: &[(std::ffi::OsString, std::ffi::OsString)],
    ) -> Result<Self, MiseError> {
        validate_toolchain_env(toolchain_env)?;
        self.extra_env.extend_from_slice(toolchain_env);
        Ok(self)
    }
}

/// Require each owned home exactly once, with matching Mise and native homes.
fn validate_toolchain_env(
    toolchain_env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Result<(), MiseError> {
    for (key, value) in toolchain_env {
        let name = key.to_string_lossy();
        if !TOOL_HOME_KEYS.contains(&name.as_ref()) || value.is_empty() {
            return Err(invalid_toolchain_env(&name));
        }
    }
    if toolchain_env.len() != TOOL_HOME_KEYS.len()
        || TOOL_HOME_KEYS.iter().any(|key| {
            toolchain_env
                .iter()
                .filter(|(seen, _)| seen.as_os_str() == OsStr::new(key))
                .count()
                != 1
        })
    {
        return Err(invalid_toolchain_env("incomplete_or_duplicate"));
    }
    for (mise_key, native_key) in [
        (MISE_RUSTUP_HOME_ENV, RUSTUP_HOME_ENV),
        (MISE_CARGO_HOME_ENV, CARGO_HOME_ENV),
    ] {
        let value = |name: &str| {
            toolchain_env
                .iter()
                .find(|(key, _)| key.as_os_str() == OsStr::new(name))
                .map(|(_, value)| value)
        };
        if value(mise_key) != value(native_key) {
            return Err(invalid_toolchain_env("mismatched_native_home"));
        }
    }
    Ok(())
}

/// Invalid controlled tool-home env input.
fn invalid_toolchain_env(value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "toolchain_env".to_owned(),
        value: value.to_owned(),
    }
}
