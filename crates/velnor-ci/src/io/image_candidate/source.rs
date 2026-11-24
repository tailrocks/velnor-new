// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::process;
use super::{DOCKERFILE, fail, regular, source_inputs};
use crate::error::CiError;
use crate::image_candidate::json::is_hex;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);

fn git(root: &Path, args: &[&str]) -> Result<String, CiError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    process::text(&mut command, GIT_TIMEOUT).map(|output| output.trim().to_owned())
}

pub(super) fn identity(
    root: &Path,
    requested: &str,
    github_sha: Option<&str>,
) -> Result<Value, CiError> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    if requested != head || !is_hex(&head, 40) {
        return fail("Requested source commit is not the same as the Git commit in the worktree.");
    }
    if !git(root, &["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
        return fail("Build only when the Git worktree is clean.");
    }
    if github_sha != Some(head.as_str()) {
        return fail("GitHub source commit is not the same as the Git commit in the worktree.");
    }
    let committed = git(root, &["show", &format!("{head}:{DOCKERFILE}")])?;
    let path = root.join(DOCKERFILE);
    regular(&path, DOCKERFILE)?;
    let current = fs::read_to_string(&path)
        .map_err(|error| CiError::new(format!("{}: {error}", path.display())))?;
    if committed != current.trim_end_matches('\n') {
        return fail(
            "Dockerfile bytes are not the same as the Dockerfile bytes in the requested source commit.",
        );
    }
    source_inputs(root)
}
