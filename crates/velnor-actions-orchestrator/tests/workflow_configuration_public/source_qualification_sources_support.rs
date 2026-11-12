// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH as CI_WORKFLOW_PATH;

use super::source_qualification::{WORKFLOW_PATH, generator_repo};
use super::workflow_configuration_fixture::{
    add_independent_mbx_package, git, write_nextest_task, write_root_cargo_lock,
};

pub(super) const RUNNER_ROOT: &str = "crates/velnor-runner";
pub(super) const RUNNER_TEMP_EXPR: &str = "${{ runner.temp }}";
pub(super) const PREPARE_PHASE: &str = "prepare-external-evidence-directories";
pub(super) const PREFETCH_PHASE: &str = "prefetch-locked-sources-for-qualification-workspaces";

pub(super) struct Rendered {
    pub(super) ci: Value,
    pub(super) sq: Value,
}

pub(super) fn sources_repo() -> Result<TempDir, Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let root = repo.path();
    write_root_cargo_lock(root, &["velnor-actions-cli"])?;
    add_independent_mbx_package(root, RUNNER_ROOT, "velnor-runner")?;
    write_nextest_task(root)?;
    write_nextest_task(&root.join(RUNNER_ROOT))?;
    git(&["add", "-A"], root)?;
    Ok(repo)
}

pub(super) fn render(repo: &Path) -> Result<Rendered, Box<dyn Error>> {
    let tree = render_staged_tree(&prepare(repo)?)?;
    let parse = |path: &str| -> Result<Value, Box<dyn Error>> {
        Ok(serde_saphyr::from_str(
            tree.get(path).ok_or_else(|| format!("{path} missing"))?,
        )?)
    };
    Ok(Rendered {
        ci: parse(CI_WORKFLOW_PATH)?,
        sq: parse(WORKFLOW_PATH)?,
    })
}

pub(super) fn steps_of<'a>(
    workflow: &'a Value,
    job: &str,
) -> Result<&'a Vec<Value>, Box<dyn Error>> {
    workflow["jobs"][job]["steps"]
        .as_array()
        .ok_or_else(|| format!("job {job} has no steps").into())
}

pub(super) fn producer_ids(workflow: &Value) -> Vec<String> {
    ["ordinary-harness", "keep-harness", "generation-checks"]
        .into_iter()
        .filter(|id| workflow["jobs"][*id].is_object())
        .map(str::to_owned)
        .collect()
}

pub(super) fn gather_ids(workflow: &Value) -> Vec<String> {
    ["source-qualification"]
        .into_iter()
        .filter(|id| workflow["jobs"][*id].is_object())
        .map(str::to_owned)
        .collect()
}

pub(super) fn positions(steps: &[Value], predicate: impl Fn(&Value) -> bool) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| predicate(step))
        .map(|(index, _)| index)
        .collect()
}

pub(super) fn only(found: &[usize], what: &str, job: &str) -> Result<usize, Box<dyn Error>> {
    match found {
        [index] => Ok(*index),
        other => Err(format!("job {job} must have exactly one {what}: {other:?}").into()),
    }
}

pub(super) fn uses_prefix(step: &Value, prefix: &str) -> bool {
    step["uses"]
        .as_str()
        .is_some_and(|uses| uses.starts_with(prefix))
}

pub(super) fn runs_phase(step: &Value, phase: &str) -> bool {
    step["run"].as_str().is_some_and(|run| run.ends_with(phase))
}

pub(super) fn output_refs(text: &str) -> Vec<(String, String)> {
    let name_chars = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut refs = Vec::new();
    for (start, _) in text.match_indices("steps.") {
        let rest = &text[start + "steps.".len()..];
        let id_end = rest.find(|c: char| !name_chars(c)).unwrap_or(rest.len());
        let Some(after) = rest[id_end..].strip_prefix(".outputs.") else {
            continue;
        };
        let name_end = after.find(|c: char| !name_chars(c)).unwrap_or(after.len());
        refs.push((rest[..id_end].to_owned(), after[..name_end].to_owned()));
    }
    refs
}

pub(super) fn text<'a>(value: &'a Value, what: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .as_str()
        .ok_or_else(|| format!("{what} is not text").into())
}
