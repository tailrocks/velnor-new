// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock};

pub(crate) fn workspace_root_for_manifest<'a>(
    manifest: &str,
    fetch_roots: &'a [String],
) -> &'a str {
    let mut matching = "";
    for root in fetch_roots {
        if !root.is_empty()
            && (manifest == root || manifest.starts_with(&format!("{root}/")))
            && root.len() > matching.len()
        {
            matching = root;
        }
    }
    matching
}

pub(crate) fn crate_fetch_roots(
    manifest: &str,
    fetch_roots: &[String],
    needs_all_workspaces: bool,
) -> Vec<String> {
    if needs_all_workspaces {
        return fetch_roots.to_vec();
    }
    let root = workspace_root_for_manifest(manifest, fetch_roots);
    fetch_roots
        .iter()
        .filter(|candidate| candidate.as_str() == root)
        .cloned()
        .collect()
}

pub(crate) fn lockful_roots(root: &Path, workspaces: &[PlannedWorkspace]) -> Vec<String> {
    let mut roots = Vec::new();
    for workspace in workspaces {
        let lock = workspace_lock(&workspace.record.workspace_root);
        if root.join(&lock).is_file() {
            roots.push(workspace.record.workspace_root.clone());
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

pub(crate) fn fetch_steps_for_crate(
    catalog: &ToolCatalog,
    roots: &[String],
    all_features: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    fetch_steps_with(catalog, roots, &env, false, all_features)
}

pub(crate) fn fetch_steps_for_preview(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Vec<Step>, OrchestratorError> {
    fetch_steps_with(catalog, roots, env, true, false)
}

fn fetch_steps_with(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
    remove_credentials: bool,
    all_features: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    velnor_actions_workflow_renderer::plan_sources::cargo_fetch::fetch_steps_with(
        catalog,
        roots,
        env,
        remove_credentials,
        all_features,
    )
    .map_err(|err| match err {
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Contract(err) => {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        }
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::ShellRender(err) => {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        }
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheSourcesPolicy(
            problem,
        ) => OrchestratorError::Contract { problem },
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheActionRef(
            problem,
        ) => OrchestratorError::WorkflowLint { problem },
        velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Render(err) => {
            OrchestratorError::from(err)
        }
    })
}

pub(crate) fn validate_root(root: &str) -> Result<(), OrchestratorError> {
    velnor_actions_contract::validate_fetch_root(root).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}
