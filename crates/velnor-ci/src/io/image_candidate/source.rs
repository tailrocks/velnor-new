// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::process::Runner;
use super::{DOCKERFILE, MAX_INPUT_BYTES, fail, regular, source_inputs};
use crate::error::CiError;
use crate::image_candidate::json::is_hex;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LINE_BYTES: u64 = 1 << 20;

fn git(runner: &Runner, root: &Path, args: &[&str], limit: u64) -> Result<String, CiError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    runner.text_up_to(&mut command, GIT_TIMEOUT, limit)
}

pub(super) fn git_line(runner: &Runner, root: &Path, args: &[&str]) -> Result<String, CiError> {
    git(runner, root, args, MAX_LINE_BYTES).map(|output| output.trim().to_owned())
}

pub(super) fn identity(
    runner: &Runner,
    root: &Path,
    requested: &str,
    github_sha: Option<&str>,
) -> Result<Value, CiError> {
    let top = git_line(runner, root, &["rev-parse", "--show-toplevel"])?;
    if fs::canonicalize(&top).ok().as_deref() != Some(root) {
        return fail("--root must be the top directory of the Git worktree.");
    }
    let head = git_line(runner, root, &["rev-parse", "HEAD"])?;
    if requested != head || !is_hex(&head, 40) {
        return fail("Requested source commit is not the same as the Git commit in the worktree.");
    }
    let status = ["status", "--porcelain", "--untracked-files=all"];
    if !git_line(runner, root, &status)?.is_empty() {
        return fail("Build only when the Git worktree is clean.");
    }
    if github_sha != Some(head.as_str()) {
        return fail("GitHub source commit is not the same as the Git commit in the worktree.");
    }
    let blob = ["show", &format!("{head}:{DOCKERFILE}")];
    let committed = git(runner, root, &blob, MAX_INPUT_BYTES)?;
    let path = root.join(DOCKERFILE);
    if regular(&path, DOCKERFILE)? > MAX_INPUT_BYTES {
        return fail(format!("{DOCKERFILE} exceeds its size limit."));
    }
    let current = fs::read_to_string(&path)
        .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
    if committed != current {
        return fail(
            "Dockerfile bytes are not the same as the Dockerfile bytes in the requested source commit.",
        );
    }
    source_inputs(root)
}
