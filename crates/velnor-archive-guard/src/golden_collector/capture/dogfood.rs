// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::super::{CollectorError, filesystem};
use super::Context;
use super::fixture_git;
use super::output;
use velnor_actions_mise::{BoundedCommandOutput, BoundedCommandStatus};

pub(super) fn capture(context: &Context<'_>, out: &Path) -> Result<bool, CollectorError> {
    let preview = out.join("preview");
    let raw = context.workspace.join("raw/dogfood");
    fs::create_dir_all(&preview).map_err(CollectorError::operational)?;
    fs::create_dir_all(&raw).map_err(CollectorError::operational)?;
    let head = root_head(context)?;
    let plan = run_candidate(context, "plan", None, &raw, out, "plan")?;
    require_success("dogfood plan", &plan)?;
    let root_bytes = context.root.as_os_str().as_bytes();
    let work_bytes = context.workspace.as_os_str().as_bytes();
    let mappings = [
        (root_bytes.to_vec(), b"<root>".to_vec()),
        (work_bytes.to_vec(), b"<work>".to_vec()),
    ];
    fs::write(
        out.join("plan.stderr.txt"),
        filesystem::normalize_plan_stderr(&plan.stderr, &mappings),
    )
    .map_err(CollectorError::operational)?;
    fs::write(
        out.join("plan.txt"),
        filesystem::normalize_plan(&plan.stdout, head.as_bytes(), root_bytes),
    )
    .map_err(CollectorError::operational)?;
    retain_raw_plan(&raw, &plan.stdout)?;

    let generate = run_candidate(
        context,
        "generate",
        Some(preview.as_os_str()),
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
    require_success("dogfood generate", &generate)?;
    let generated = preview.join(".github");
    require_real_directory(&generated)?;
    let expected = filesystem::read_tree(&context.root.join(".github"))?;
    let actual = filesystem::read_tree(&generated)?;
    let differences = filesystem::compare(&expected, &actual);
    let identical = differences.is_empty()
        && filesystem::link_identity(&expected) == filesystem::link_identity(&actual);
    fs::write(
        out.join("dogfood.verdict"),
        if identical {
            "identical\n"
        } else {
            "DIFFERS\n"
        },
    )
    .map_err(CollectorError::operational)?;
    let mut diff = differences.join("\n");
    if !diff.is_empty() {
        diff.push('\n');
    }
    fs::write(out.join("dogfood.diff"), diff).map_err(CollectorError::operational)?;
    let tree = filesystem::read_tree(&preview)?;
    fs::write(out.join("tree.sha256"), filesystem::tree_hash(&tree))
        .map_err(CollectorError::operational)?;
    Ok(identical)
}

fn root_head(context: &Context<'_>) -> Result<String, CollectorError> {
    let output = fixture_git::run(context, "dogfood", context.root, &["rev-parse", "HEAD"])?;
    let value = String::from_utf8(output.stdout).map_err(CollectorError::operational)?;
    let value = value.trim_end();
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CollectorError::operational(
            "dogfood git returned a malformed commit",
        ));
    }
    Ok(value.to_owned())
}

fn run_candidate(
    context: &Context<'_>,
    operation: &str,
    output: Option<&std::ffi::OsStr>,
    raw: &Path,
    out: &Path,
    name: &str,
) -> Result<BoundedCommandOutput, CollectorError> {
    let mut command = context.cli.command();
    command.current_dir(context.root);
    if operation == "plan" {
        command.arg("plan");
    } else {
        command.arg("generate").arg("--output-dir");
        if let Some(output) = output {
            command.arg(output);
        }
    }
    context
        .cli
        .run_candidate(context.guard, context.root, &mut command, |result| {
            output::persist_result(raw, name, out, result)
        })
}

fn require_success(operation: &str, output: &BoundedCommandOutput) -> Result<(), CollectorError> {
    if output.status == BoundedCommandStatus::Exited(0) {
        return Ok(());
    }
    Err(CollectorError::operational(format!(
        "{operation} failed ({:?}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim_end()
    )))
}

fn retain_raw_plan(raw: &Path, stdout: &[u8]) -> Result<(), CollectorError> {
    if std::env::var("VELNOR_GOLDEN_KEEP_WORK").as_deref() == Ok("1") {
        fs::write(raw.join("plan.source-raw.txt"), stdout).map_err(CollectorError::operational)?;
    }
    Ok(())
}

fn require_real_directory(path: &Path) -> Result<(), CollectorError> {
    let metadata = fs::symlink_metadata(path).map_err(CollectorError::operational)?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(CollectorError::operational(
            "dogfood generate output has no real .github directory",
        ))
    }
}
