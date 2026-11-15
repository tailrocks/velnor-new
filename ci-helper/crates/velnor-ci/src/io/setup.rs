// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use velnor_gen::io::read_files;
use velnor_rust::{mise_files, tool_pins};

use super::{append_outputs, append_to, current_dir};
use crate::error::CiError;
use crate::setup::{check_tools, env_lines, output_lines, parse_reported};

const ENV_VARIABLE: &str = "GITHUB_ENV";

pub(super) fn prepare() -> Result<String, CiError> {
    let root = current_dir()?;
    let files = read_files(&root, &mise_files()).map_err(|e| CiError::new(e.to_string()))?;
    let pins = tool_pins(&files).map_err(|e| CiError::new(e.to_string()))?;
    let output = Command::new("mise")
        .args(["ls", "--current", "--json"])
        .current_dir(&root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run mise: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "mise ls failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    check_tools(
        &pins,
        &parse_reported(&String::from_utf8_lossy(&output.stdout))?,
    )?;
    append_to(ENV_VARIABLE, &env_lines())?;
    append_outputs(&output_lines())?;
    Ok(String::new())
}

pub(super) fn check_metadata() -> Result<String, CiError> {
    velnor_gen::io::check_metadata(&current_dir()?).map_err(|e| CiError::new(e.to_string()))?;
    Ok(String::new())
}
