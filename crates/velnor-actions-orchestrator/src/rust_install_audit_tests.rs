// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_contract::workflow::TOOLS_PRODUCER_INSTALL_CONDITION;
use velnor_actions_contract::workflow::crate_job::TOOLS_PRODUCER_JOB_ID_PREFIX;
use velnor_actions_workflow_renderer::render::{RenderedFile, WORKFLOW_PATH};

use super::audit_with_verifiers;
use crate::OrchestratorError;
use crate::git_fixture;
use crate::prepare::prepare_for_candidate as prepare;
use crate::workflow::verifier_actions::VerifierActionSet;

const RUST_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"default\"\nfeatures = []\nall_features = false\ntarget = \"host\"\n";
const INSTALLED_VERIFIER: &str = "./.github/actions/velnor-verify-rust-manifest";
const RESTORED_VERIFIER: &str = "./.github/actions/velnor-verify-restored-rust";
const VERIFIER_NAME: &str = "Verify Rust channel manifest";
const RESTORED_NAME: &str = "Verify restored Rust toolchain";
const PREPARE_NAME: &str = "Prepare pinned tools";
const INDENT: &str = "        ";

struct Rendered {
    files: Vec<RenderedFile>,
    verifiers: VerifierActionSet,
}

pub(crate) fn make_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let init = git_fixture::sterile_command()
        .current_dir(root)
        .args(["init", "-b", "testmain"])
        .output()?;
    if !init.status.success() {
        return Err(String::from_utf8_lossy(&init.stderr).into_owned().into());
    }
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), RUST_CONFIG)?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    write_task(root)?;
    Ok(dir)
}

fn write_task(root: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    fs::create_dir_all(root.join(".mise/tasks"))?;
    let task = root.join(".mise/tasks/test");
    fs::write(&task, "#!/bin/sh\ncargo nextest run --locked\n")?;
    fs::set_permissions(&task, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn rendered() -> Result<Rendered, Box<dyn Error>> {
    let repo = make_repo()?;
    let prep = prepare(repo.path())?;
    let tree = crate::generate::render_staged_tree(&prep)?;
    let verifiers = VerifierActionSet::build(&prep.emission_catalog(), env!("CARGO_PKG_VERSION"))?;
    Ok(Rendered {
        files: tree.files,
        verifiers,
    })
}

struct JobBlock {
    id: String,
    start: usize,
    end: usize,
}

fn job_blocks(text: &str) -> Vec<JobBlock> {
    let mut blocks: Vec<JobBlock> = Vec::new();
    let mut offset = 0;
    let mut in_jobs = false;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end();
        if bare == "jobs:" {
            in_jobs = true;
        } else if in_jobs
            && !bare.starts_with("   ")
            && bare.starts_with("  ")
            && bare.ends_with(':')
        {
            if let Some(last) = blocks.last_mut() {
                last.end = offset;
            }
            blocks.push(JobBlock {
                id: bare.trim().trim_end_matches(':').to_owned(),
                start: offset,
                end: text.len(),
            });
        }
        offset += line.len();
    }
    blocks
}

fn workflow_text(rendered: &Rendered) -> Result<&str, Box<dyn Error>> {
    rendered
        .files
        .iter()
        .find(|file| file.path == WORKFLOW_PATH)
        .map(|file| file.bytes.as_str())
        .ok_or_else(|| "rendered workflow is missing".into())
}

fn edited(
    rendered: &Rendered,
    job_id: &str,
    from: &str,
    to: &str,
) -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    edited_all(rendered, job_id, &[(from, to)])
}

fn edited_all(
    rendered: &Rendered,
    job_id: &str,
    edits: &[(&str, &str)],
) -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    let text = workflow_text(rendered)?;
    let block = job_blocks(text)
        .into_iter()
        .find(|block| block.id == job_id)
        .ok_or_else(|| format!("no job block {job_id}"))?;
    let mut body = text[block.start..block.end].to_owned();
    for (from, to) in edits {
        let hits = body.matches(from).count();
        if hits != 1 {
            return Err(
                format!("{job_id}: expected one occurrence of {from:?}, found {hits}").into(),
            );
        }
        body = body.replacen(from, to, 1);
    }
    let changed = format!("{}{}{}", &text[..block.start], body, &text[block.end..]);
    Ok(rendered
        .files
        .iter()
        .map(|file| RenderedFile {
            path: file.path.clone(),
            bytes: if file.path == WORKFLOW_PATH {
                changed.clone()
            } else {
                file.bytes.clone()
            },
        })
        .collect())
}

fn first_job_containing(rendered: &Rendered, needle: &str) -> Result<String, Box<dyn Error>> {
    let text = workflow_text(rendered)?;
    job_blocks(text)
        .into_iter()
        .find(|block| text[block.start..block.end].contains(needle))
        .map(|block| block.id)
        .ok_or_else(|| format!("no job contains {needle:?}").into())
}

fn gated_verifier_lines() -> String {
    format!(
        "name: {VERIFIER_NAME}\n{INDENT}if: {TOOLS_PRODUCER_INSTALL_CONDITION}\n{INDENT}uses: {INSTALLED_VERIFIER}"
    )
}

fn gated_install_lines() -> String {
    format!("name: {PREPARE_NAME}\n{INDENT}if: {TOOLS_PRODUCER_INSTALL_CONDITION}\n")
}

fn producer_with_gated_verifier(rendered: &Rendered) -> Result<String, Box<dyn Error>> {
    let id = first_job_containing(rendered, &gated_verifier_lines())?;
    assert!(
        id.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX),
        "gated installed verifier first appears in {id}"
    );
    let text = workflow_text(rendered)?;
    let block = job_blocks(text)
        .into_iter()
        .find(|block| block.id == id)
        .ok_or("producer block vanished")?;
    assert!(
        text[block.start..block.end].contains(&gated_install_lines()),
        "{id} has no gated install step"
    );
    Ok(id)
}

fn assert_rejected(
    result: Result<(), OrchestratorError>,
    owner: &str,
) -> Result<(), Box<dyn Error>> {
    let message = result
        .err()
        .ok_or("the audit accepted a tampered tree")?
        .to_string();
    assert!(
        message.contains("rust_install_unverified:"),
        "wrong error: {message}"
    );
    assert!(
        message.contains(&format!(":{owner}:")),
        "error does not name {owner}: {message}"
    );
    Ok(())
}

#[test]
fn audit_accepts_generated_producer_shape() -> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    producer_with_gated_verifier(&rendered)?;
    audit_with_verifiers(&rendered.files, &rendered.verifiers)?;
    Ok(())
}

#[test]
fn audit_rejects_gated_verifier_after_ungated_install() -> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    let producer = producer_with_gated_verifier(&rendered)?;
    let files = edited(
        &rendered,
        &producer,
        &gated_install_lines(),
        &format!("name: {PREPARE_NAME}\n"),
    )?;
    assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), &producer)
}

#[test]
fn audit_rejects_verifier_with_other_condition() -> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    let producer = producer_with_gated_verifier(&rendered)?;
    let gate = format!("if: {TOOLS_PRODUCER_INSTALL_CONDITION}");
    for other in [
        "if: always()",
        "if: steps.tools-restore.outputs.cache-hit != 'true'",
        "if: steps.v2.outputs.enabled == 'true' && steps.tools-restore.outputs.cache-hit == 'true'",
    ] {
        let files = edited(
            &rendered,
            &producer,
            &gated_verifier_lines(),
            &gated_verifier_lines().replace(&gate, other),
        )?;
        assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), &producer)?;
    }
    let files = edited(
        &rendered,
        &producer,
        &gated_install_lines(),
        &gated_install_lines().replace(&gate, "if: always()"),
    )?;
    assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), &producer)
}

#[test]
fn audit_rejects_producer_condition_outside_a_producer_job() -> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    let producer = producer_with_gated_verifier(&rendered)?;
    let files = edited(
        &rendered,
        &producer,
        &format!("  {producer}:\n"),
        "  crate-x:\n",
    )?;
    assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), "crate-x")
}

#[test]
fn audit_rejects_ungated_verifier_after_gated_install() -> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    let producer = producer_with_gated_verifier(&rendered)?;
    let files = edited(
        &rendered,
        &producer,
        &gated_verifier_lines(),
        &format!("name: {VERIFIER_NAME}\n{INDENT}uses: {INSTALLED_VERIFIER}"),
    )?;
    assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), &producer)
}

#[test]
fn audit_rejects_gated_restored_verifier_everywhere() -> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    let restored = format!("name: {RESTORED_NAME}\n{INDENT}uses: {RESTORED_VERIFIER}");
    let crate_job = first_job_containing(&rendered, &restored)?;
    assert!(
        !crate_job.starts_with(TOOLS_PRODUCER_JOB_ID_PREFIX),
        "restored verifier first appears in {crate_job}"
    );
    let files = edited(
        &rendered,
        &crate_job,
        &restored,
        &format!(
            "name: {RESTORED_NAME}\n{INDENT}if: {TOOLS_PRODUCER_INSTALL_CONDITION}\n{INDENT}uses: {RESTORED_VERIFIER}"
        ),
    )?;
    assert_rejected(
        audit_with_verifiers(&files, &rendered.verifiers),
        &crate_job,
    )?;
    let producer = producer_with_gated_verifier(&rendered)?;
    let files = edited(
        &rendered,
        &producer,
        &gated_verifier_lines(),
        &format!(
            "name: {RESTORED_NAME}\n{INDENT}if: {TOOLS_PRODUCER_INSTALL_CONDITION}\n{INDENT}uses: {RESTORED_VERIFIER}"
        ),
    )?;
    assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), &producer)
}

#[test]
fn install_audit_recognises_bracket_form_rust_installs_in_every_quoting()
-> Result<(), Box<dyn Error>> {
    let rendered = rendered()?;
    let verifier_step = format!(
        "      - name: {VERIFIER_NAME}\n{INDENT}uses: {INSTALLED_VERIFIER} # zizmor: ignore[self-repository]\n"
    );
    for quoting in ["'rust[", "rust[", "\"rust[", "\\\"rust["] {
        let files = edited_all(
            &rendered,
            "plan",
            &[
                ("install 'rust[", &format!("install {quoting}")),
                (&verifier_step, ""),
            ],
        )?;
        assert_rejected(audit_with_verifiers(&files, &rendered.verifiers), "plan")?;
    }
    Ok(())
}
