// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::{CollectorError, filesystem};
use super::Context;
use super::fixture_git;
use super::manifest;
use super::output;
use velnor_actions_mise::{BoundedCommandOutput, BoundedCommandStatus};

pub(super) fn setup(
    context: &Context<'_>,
    name: &str,
    repo: &Path,
) -> Result<String, CollectorError> {
    fs::create_dir(repo).map_err(CollectorError::operational)?;
    filesystem::copy_contents(&context.root.join("fixtures").join(name), repo)?;
    manifest::prepare(context, repo)?;
    fixture_git::setup(context, name, repo)
}

pub(super) fn capture(
    context: &Context<'_>,
    name: &str,
    repo: &Path,
    head: &str,
    out: &Path,
) -> Result<(), CollectorError> {
    let preview = out.join("preview");
    let raw = context.workspace.join("raw").join(name);
    fs::create_dir_all(&preview).map_err(CollectorError::operational)?;
    fs::create_dir_all(&raw).map_err(CollectorError::operational)?;
    let plan = run_candidate(context, repo, &["plan"], None, &raw, out, "plan")?;
    require_success("plan", name, &plan)?;
    fs::write(
        out.join("plan.stderr.txt"),
        normalize_stderr(context, &plan.stderr, repo),
    )
    .map_err(CollectorError::operational)?;
    fs::write(
        out.join("plan.txt"),
        filesystem::normalize_plan(&plan.stdout, head.as_bytes(), &[]),
    )
    .map_err(CollectorError::operational)?;
    retain_raw_plan(&raw, &plan.stdout)?;

    let output_path = preview.as_os_str().to_owned();
    let generate = run_candidate(
        context,
        repo,
        &["generate", "--output-dir"],
        Some(output_path.into()),
        &raw,
        out,
        "generate",
    )?;
    fs::write(out.join("generate.stdout.txt"), &generate.stdout)
        .map_err(CollectorError::operational)?;
    fs::write(
        out.join("generate.stderr.txt"),
        filesystem::normalize_generate_stderr(&generate.stderr),
    )
    .map_err(CollectorError::operational)?;
    require_success("generate", name, &generate)?;
    let github = preview.join(".github");
    require_real_directory(
        &github,
        "candidate generate output has no real .github directory",
    )?;
    validate_bindings(context, repo, &github, &raw)?;
    let manifest_path = repo.join(".velnor/release-manifest.json");
    filesystem::replace_regular_files(&github, &manifest::replacements(&manifest_path)?)?;
    let tree = filesystem::read_tree(&preview)?;
    fs::write(out.join("tree.sha256"), filesystem::tree_hash(&tree))
        .map_err(CollectorError::operational)?;
    Ok(())
}

fn run_candidate(
    context: &Context<'_>,
    repo: &Path,
    args: &[&str],
    final_arg: Option<PathBuf>,
    raw: &Path,
    out: &Path,
    name: &str,
) -> Result<BoundedCommandOutput, CollectorError> {
    let mut command = Command::new(context.cli);
    command.current_dir(repo).args(args);
    if let Some(path) = final_arg {
        command.arg(path);
    }
    fixture_git::isolate_git_environment(&mut command);
    context
        .guard
        .run_candidate(context.root, &mut command, |output| {
            output::persist_result(raw, name, out, output)
        })
}

fn validate_bindings(
    context: &Context<'_>,
    repo: &Path,
    github: &Path,
    raw: &Path,
) -> Result<(), CollectorError> {
    let output = context.guard.run_candidate(
        context.root,
        Command::new(context.guard.path())
            .arg("qualification-output-bindings")
            .arg(repo.join(".velnor/release-manifest.json"))
            .arg(github),
        |output| output::persist_result(raw, "bindings", raw, output),
    )?;
    if output.status == BoundedCommandStatus::Exited(0) {
        Ok(())
    } else {
        Err(CollectorError::operational(format!(
            "fixture output bindings failed ({:?}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim_end()
        )))
    }
}

fn normalize_stderr(context: &Context<'_>, raw: &[u8], repo: &Path) -> Vec<u8> {
    let mappings = [
        (repo.as_os_str().as_bytes().to_vec(), b"<repo>".to_vec()),
        (
            context.root.as_os_str().as_bytes().to_vec(),
            b"<root>".to_vec(),
        ),
        (
            context.workspace.as_os_str().as_bytes().to_vec(),
            b"<work>".to_vec(),
        ),
    ];
    filesystem::normalize_plan_stderr(raw, &mappings)
}

fn require_success(
    operation: &str,
    fixture: &str,
    output: &BoundedCommandOutput,
) -> Result<(), CollectorError> {
    if output.status == BoundedCommandStatus::Exited(0) {
        return Ok(());
    }
    Err(CollectorError::operational(format!(
        "{operation} failed for {fixture} ({:?}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim_end()
    )))
}

fn retain_raw_plan(raw: &Path, stdout: &[u8]) -> Result<(), CollectorError> {
    if env::var("VELNOR_GOLDEN_KEEP_WORK").as_deref() == Ok("1") {
        fs::write(raw.join("plan.source-raw.txt"), stdout).map_err(CollectorError::operational)?;
    }
    Ok(())
}

fn require_real_directory(path: &Path, message: &str) -> Result<(), CollectorError> {
    let metadata = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(CollectorError::operational(message))
    }
}
