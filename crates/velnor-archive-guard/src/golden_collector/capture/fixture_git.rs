// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::Path;
use std::process::{Command, Output};

use super::super::CollectorError;

pub(super) fn run(root: &Path, args: &[&str]) -> Result<Output, CollectorError> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "filter.lfs.clean=git-lfs clean -- %f",
            "-c",
            "filter.lfs.smudge=git-lfs smudge -- %f",
            "-c",
            "filter.lfs.process=git-lfs filter-process",
            "-c",
            "filter.lfs.required=true",
        ])
        .args(args);
    isolate_git_environment(&mut command);
    let output = command.output().map_err(CollectorError::operational)?;
    if !output.status.success() {
        return Err(CollectorError::operational(format!(
            "fixture git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim_end()
        )));
    }
    Ok(output)
}

pub(super) fn isolate_git_environment(command: &mut Command) {
    for (key, _) in env::vars_os().filter(|(key, _)| key.as_encoded_bytes().starts_with(b"GIT_")) {
        command.env_remove(key);
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_CONFIG_SYSTEM", "/dev/null");
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command.env("GIT_TEMPLATE_DIR", "");
    command.env("GIT_AUTHOR_NAME", "v");
    command.env("GIT_AUTHOR_EMAIL", "v@v");
    command.env("GIT_COMMITTER_NAME", "v");
    command.env("GIT_COMMITTER_EMAIL", "v@v");
    command.env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z");
    command.env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z");
}

pub(super) fn setup(root: &Path) -> Result<String, CollectorError> {
    run(root, &["init", "-q"])?;
    run(root, &["add", "-A"])?;
    run(root, &["commit", "-qm", "golden"])?;
    let output = run(root, &["rev-parse", "HEAD"])?;
    let head = String::from_utf8(output.stdout).map_err(CollectorError::operational)?;
    let head = head.trim_end();
    if head.len() != 40 || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CollectorError::operational(
            "fixture git returned a malformed commit",
        ));
    }
    Ok(head.to_owned())
}
