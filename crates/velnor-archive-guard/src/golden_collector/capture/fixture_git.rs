// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::super::CollectorError;
use super::Context;
use super::output;
use velnor_actions_mise::{BoundedCommandOutput, BoundedCommandStatus};

pub(super) fn run(
    context: &Context<'_>,
    fixture: &str,
    root: &Path,
    args: &[&str],
) -> Result<BoundedCommandOutput, CollectorError> {
    let raw = context.workspace.join("raw").join(fixture).join("git");
    fs::create_dir_all(&raw).map_err(CollectorError::operational)?;
    let operation = args
        .first()
        .copied()
        .ok_or_else(|| CollectorError::operational("fixture git command has no operation"))?;
    let artifact = format!("git-{operation}");
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
    let output = context
        .guard
        .run_candidate(context.root, &mut command, |result| {
            output::persist_result(&raw, &artifact, &raw, result)
        })?;
    if output.status != BoundedCommandStatus::Exited(0) {
        return Err(CollectorError::operational(format!(
            "fixture git command failed: {}",
            format_status(&output)
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

pub(super) fn setup(
    context: &Context<'_>,
    fixture: &str,
    root: &Path,
) -> Result<String, CollectorError> {
    run(context, fixture, root, &["init", "-q"])?;
    run(context, fixture, root, &["add", "-A"])?;
    run(context, fixture, root, &["commit", "-qm", "golden"])?;
    let output = run(context, fixture, root, &["rev-parse", "HEAD"])?;
    let head = String::from_utf8(output.stdout).map_err(CollectorError::operational)?;
    let head = head.trim_end();
    if head.len() != 40 || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CollectorError::operational(
            "fixture git returned a malformed commit",
        ));
    }
    Ok(head.to_owned())
}

fn format_status(output: &BoundedCommandOutput) -> String {
    let mut details = vec![format!("status {:?}", output.status)];
    if let Some(error) = &output.execution_error {
        details.push(format!("execution error: {error}"));
    }
    if let Some(error) = &output.cleanup_error {
        details.push(format!("cleanup error: {error}"));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        details.push(format!("stderr: {}", stderr.trim_end()));
    }
    details.join("; ")
}
