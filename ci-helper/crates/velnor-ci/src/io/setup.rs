// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use velnor_gen::io::read_files;
use velnor_model::{ToolPins, Toolchain};
use velnor_rust::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, mise_files, tool_pins, toolchain};

use super::{append_outputs, append_to, current_dir};
use crate::error::CiError;
use crate::setup::{
    Missing, check_tools, env_lines, missing_toolchain, output_lines, parse_reported,
};

const ENV_VARIABLE: &str = "GITHUB_ENV";

pub(super) fn prepare() -> Result<String, CiError> {
    let root = current_dir()?;
    let files = read_files(&root, &mise_files()).map_err(|e| CiError::new(e.to_string()))?;
    let pins = tool_pins(&files).map_err(|e| CiError::new(e.to_string()))?;
    if !pins.tools.is_empty() {
        check_pinned_tools(&root, &pins)?;
    }
    let declared = read_files(&root, &[TOOLCHAIN_PLAIN, TOOLCHAIN_TOML])
        .map_err(|e| CiError::new(e.to_string()))?;
    if let Some(declared) = toolchain(&declared)
        .map_err(|e| CiError::new(e.to_string()))?
        .filter(|declared| declared.path.is_none())
    {
        install_declared(&root, &declared)?;
    }
    append_to(ENV_VARIABLE, &env_lines())?;
    append_outputs(&output_lines())?;
    Ok(String::new())
}

fn check_pinned_tools(root: &Path, pins: &ToolPins) -> Result<(), CiError> {
    let output = Command::new("mise")
        .args(["ls", "--current", "--json"])
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run mise: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "mise ls failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    check_tools(
        pins,
        &parse_reported(&String::from_utf8_lossy(&output.stdout))?,
    )
}

fn install_declared(root: &std::path::Path, declared: &Toolchain) -> Result<(), CiError> {
    let host = host_triple(root)?;
    let survey = || -> Result<Missing, CiError> {
        Ok(missing_toolchain(
            declared,
            &host,
            &tool_output(root, "rustup", &["component", "list"])?,
            &tool_output(root, "rustup", &["target", "list", "--installed"])?,
        ))
    };
    let missing = survey()?;
    if missing.is_empty() {
        return Ok(());
    }
    for (kind, names) in [
        ("component", &missing.components),
        ("target", &missing.targets),
    ] {
        if !names.is_empty() {
            let mut args = vec![kind, "add"];
            args.extend(names.iter().map(String::as_str));
            tool_output(root, "rustup", &args)?;
        }
    }
    survey()?.ensure_none()
}

fn tool_output(root: &std::path::Path, program: &str, args: &[&str]) -> Result<String, CiError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run {program}: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn host_triple(root: &std::path::Path) -> Result<String, CiError> {
    tool_output(root, "rustc", &["-vV"])?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| CiError::new("rustc -vV does not report a host".to_owned()))
}

pub(super) fn check_metadata() -> Result<String, CiError> {
    velnor_gen::io::check_metadata(&current_dir()?).map_err(|e| CiError::new(e.to_string()))?;
    Ok(String::new())
}
