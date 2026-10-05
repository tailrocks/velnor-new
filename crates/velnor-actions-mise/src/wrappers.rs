//! Structural Cargo-wrapper inspection for Mise configuration.
//!
//! Pure over bytes the orchestrator supplies: this module never reads the
//! filesystem. It structurally resolves `wrappers.cargo.command` (section,
//! dotted-key, or inline-table spelling) plus the `MBX_CARGO_SHIM_MODE`
//! env value; only the exact command `mbx` selects MBX. Comments, string
//! mentions, and near-miss commands (`not-mbx`, `mbx2`) are never
//! evidence. Malformed TOML is a [`WrapperDiagnostic`], never a guess.

use std::fmt;

use crate::toml_scan::{TomlValue, parse_toml};

/// Exact wrapper command selecting MBX (no substring matching).
pub const MBX_COMMAND: &str = "mbx";

/// Cargo-shim env key recorded alongside the wrapper command.
pub const MBX_SHIM_ENV: &str = "MBX_CARGO_SHIM_MODE";

/// Structurally resolved Cargo wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoWrapper {
    /// Exact `wrappers.cargo.command` string value.
    pub command: String,
    /// One-based line of the command assignment.
    pub line: u32,
    /// `MBX_CARGO_SHIM_MODE` value when structurally present.
    pub shim_mode: Option<String>,
}

impl CargoWrapper {
    /// Whether this wrapper selects MBX (exact `mbx` command).
    #[must_use]
    pub fn is_mbx(&self) -> bool {
        is_mbx_command(&self.command)
    }
}

/// Whether `command` selects MBX: exact `mbx` only, never a substring.
#[must_use]
pub fn is_mbx_command(command: &str) -> bool {
    command == MBX_COMMAND
}

/// Strict wrapper-inspection failure naming the offending line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperDiagnostic {
    /// One-based line of the failure (key line for value problems).
    pub line: u32,
    /// Stable problem code.
    pub problem: String,
}

impl fmt::Display for WrapperDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wrapper_invalid:{}:{}", self.line, self.problem)
    }
}

impl std::error::Error for WrapperDiagnostic {}

/// Resolve the Cargo wrapper from Mise TOML bytes.
///
/// Returns `None` when no structural `wrappers.cargo.command` exists.
///
/// # Errors
///
/// Returns [`WrapperDiagnostic`] on malformed TOML, a non-string
/// command, or a non-string shim-mode value.
pub fn parse_cargo_wrapper(content: &str) -> Result<Option<CargoWrapper>, WrapperDiagnostic> {
    let doc = parse_toml(content).map_err(|failed| WrapperDiagnostic {
        line: failed.line,
        problem: failed.problem,
    })?;
    let Some(found) = doc
        .find(&["wrappers", "cargo", "command"])
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    let TomlValue::Str(command) = &found.value else {
        return Err(WrapperDiagnostic {
            line: found.line,
            problem: "command_not_string".to_owned(),
        });
    };
    let shim = shim_mode(&doc)?;
    Ok(Some(CargoWrapper {
        command: command.clone(),
        line: found.line,
        shim_mode: shim,
    }))
}

/// Resolve the shim-mode env value, requiring a string when present.
fn shim_mode(doc: &crate::toml_scan::TomlDoc) -> Result<Option<String>, WrapperDiagnostic> {
    let found = doc.find(&["wrappers", "cargo", "env", MBX_SHIM_ENV]);
    let Some(entry) = found.into_iter().next() else {
        return Ok(None);
    };
    let TomlValue::Str(mode) = &entry.value else {
        return Err(WrapperDiagnostic {
            line: entry.line,
            problem: "shim_mode_not_string".to_owned(),
        });
    };
    Ok(Some(mode.clone()))
}
