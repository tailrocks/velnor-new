// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::toml_scan::{TomlValue, parse_toml};

pub const MBX_COMMAND: &str = "mbx";

pub const MBX_SHIM_ENV: &str = "MBX_CARGO_SHIM_MODE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoWrapper {
    pub command: String,
    pub line: u32,
    pub shim_mode: Option<String>,
}

impl CargoWrapper {
    #[must_use]
    pub fn is_mbx(&self) -> bool {
        is_mbx_command(&self.command)
    }
}

#[must_use]
pub fn is_mbx_command(command: &str) -> bool {
    command == MBX_COMMAND
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperDiagnostic {
    pub line: u32,
    pub problem: String,
}

impl fmt::Display for WrapperDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wrapper_invalid:{}:{}", self.line, self.problem)
    }
}

impl std::error::Error for WrapperDiagnostic {}

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
