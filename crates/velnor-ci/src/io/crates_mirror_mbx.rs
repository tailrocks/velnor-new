// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Fail-closed MBX routing for the Cargo mirror wrapper.
//!
//! When a repository declares MBX in its mise configuration, every `cargo` the job runs must
//! reach mise's command wrapper, which runs MBX. `crates-mirror` reads the declaration once,
//! in the workspace, and records it for later steps as `VELNOR_CARGO_MBX`. The wrapper then
//! refuses any plain `cargo` it finds on PATH instead of running it.

use std::path::Path;

use crate::error::CiError;

use super::crates_mirror::failed;

/// Environment variable that marks the job as one whose repository declares MBX.
pub(super) const ENV: &str = "VELNOR_CARGO_MBX";
pub(super) const VALUE: &str = "required";
const FILES: [&str; 2] = ["mise.toml", ".mise.toml"];

pub(super) fn required() -> bool {
    std::env::var_os(ENV).is_some_and(|value| !value.is_empty())
        || super::crates_mirror_mode::recorded().is_ok_and(|mode| mode.is_some_and(|mode| mode.mbx))
}

pub(super) fn refused(cargo: &Path) -> CiError {
    CiError::new(format!(
        "mbx is declared but cargo on PATH isn't the mise mbx wrapper: {}",
        cargo.display()
    ))
}

/// Whether the mise configuration in the current directory declares MBX.
pub(super) fn declared_here() -> Result<bool, CiError> {
    let directory = std::env::current_dir().map_err(|error| failed(&error))?;
    for name in FILES {
        let path = directory.join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                if declares(&text)
                    .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?
                {
                    return Ok(true);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CiError::new(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(false)
}

/// Whether a mise configuration routes cargo through MBX: `[wrappers.cargo] command = "mbx"`
/// or `mr_boxington = true` on the `rust` tool.
pub(super) fn declares(text: &str) -> Result<bool, CiError> {
    let table: toml::Table = text
        .parse()
        .map_err(|error: toml::de::Error| CiError::new(error.to_string()))?;
    let wrapper = table
        .get("wrappers")
        .and_then(|wrappers| wrappers.get("cargo"))
        .and_then(|cargo| cargo.get("command"))
        .and_then(toml::Value::as_str)
        == Some("mbx");
    let rust = table
        .get("tools")
        .and_then(|tools| tools.get("rust"))
        .and_then(|rust| rust.get("mr_boxington"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    Ok(wrapper || rust)
}
