// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

use super::super::{CollectorError, filesystem};
use super::Context;
use super::fixture_git;

pub(super) fn capture(context: &Context<'_>, out: &Path) -> Result<bool, CollectorError> {
    let preview = out.join("preview");
    let raw = context.workspace.join("raw/dogfood");
    fs::create_dir_all(&preview).map_err(CollectorError::operational)?;
    fs::create_dir_all(&raw).map_err(CollectorError::operational)?;
    let head = root_head(context.root)?;
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

fn root_head(root: &Path) -> Result<String, CollectorError> {
    let output = fixture_git::run(root, &["rev-parse", "HEAD"])?;
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
) -> Result<std::process::Output, CollectorError> {
    let mut command = Command::new(context.cli);
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
        .guard
        .run_candidate(context.root, &mut command, |result| {
            save_raw(raw, name, result)?;
            save_status(out, name, result)
        })
}

fn save_raw(raw: &Path, name: &str, output: &std::process::Output) -> Result<(), CollectorError> {
    fs::write(raw.join(format!("{name}.raw.txt")), &output.stdout)
        .map_err(CollectorError::operational)?;
    fs::write(raw.join(format!("{name}.stderr.raw.txt")), &output.stderr)
        .map_err(CollectorError::operational)
}

fn save_status(
    out: &Path,
    name: &str,
    output: &std::process::Output,
) -> Result<(), CollectorError> {
    fs::write(
        out.join(format!("{name}.exit")),
        format!("{}\n", output.status.code().unwrap_or(128)),
    )
    .map_err(CollectorError::operational)
}

fn require_success(operation: &str, output: &std::process::Output) -> Result<(), CollectorError> {
    if output.status.success() {
        return Ok(());
    }
    Err(CollectorError::operational(format!(
        "{operation} failed: {}",
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
