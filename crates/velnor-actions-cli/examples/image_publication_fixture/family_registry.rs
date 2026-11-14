// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, ffi::OsString, fs, path::Path, process::ExitCode};

use serde_json::Value;

pub(super) fn run(
    args: &[OsString],
    state: &mut Value,
    state_path: &Path,
) -> Result<ExitCode, Box<dyn Error>> {
    if state.get("registries").is_none() {
        return super::crane::run(args, state, state_path);
    }
    let reference_index = if args.first().is_some_and(|arg| arg == "push") {
        2
    } else {
        1
    };
    let reference = args
        .get(reference_index)
        .and_then(|arg| arg.to_str())
        .ok_or("family registry reference missing")?;
    let repository = reference
        .split_once('@')
        .or_else(|| reference.rsplit_once(':'))
        .map(|(repository, _)| repository)
        .ok_or("family registry repository missing")?;
    let selected = state["registries"]
        .get(repository)
        .ok_or("family registry repository not configured")?
        .clone();
    let original = std::mem::replace(&mut state["crane"], selected);
    let result = super::crane::run(args, state, state_path);
    state["registries"][repository] = state["crane"].clone();
    state["crane"] = original;
    fs::write(state_path, serde_json::to_vec(state)?)?;
    result
}
