// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::report::LaneMetrics;
use std::collections::BTreeSet;
use std::process::Command;

const SKIP_WORKTREE: &[u8] = b"S ";

pub(super) fn collect_hints(metrics: &mut LaneMetrics, log: &[u8]) {
    let text = String::from_utf8_lossy(log);
    if !text.contains("No such file or directory") {
        return;
    }
    match left_out() {
        Ok((top, skipped)) => metrics
            .diagnostics
            .extend(crate::sparse_hint::hints(&text, &top, &skipped)),
        Err(error) => metrics
            .diagnostics
            .push(format!("Sparse checkout hint unavailable: {error}")),
    }
}

fn git(directory: Option<&str>, args: &[&str]) -> Result<Vec<u8>, CiError> {
    let mut command = Command::new("git");
    command.args(
        directory
            .into_iter()
            .flat_map(|directory| ["-C", directory]),
    );
    let output = command
        .args(args)
        .output()
        .map_err(|error| CiError::new(format!("cannot run git: {error}")))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(CiError::new(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn left_out() -> Result<(String, BTreeSet<String>), CiError> {
    let top = String::from_utf8_lossy(&git(None, &["rev-parse", "--show-toplevel"])?)
        .trim_end_matches(['\n', '\r'])
        .to_owned();
    let listing = git(Some(&top), &["ls-files", "-v", "-z"])?;
    let skipped = listing
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.strip_prefix(SKIP_WORKTREE))
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect();
    Ok((top, skipped))
}
