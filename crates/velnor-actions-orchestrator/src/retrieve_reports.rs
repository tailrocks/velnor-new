// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "retrieve_reports/staged.rs"]
mod staged;

use velnor_actions_contract::staged_reads::read_staged_text;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{parse_strict_json, validate_artifact_id};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::retrieve_retry::download_with_retry;

pub(crate) use staged::read_staged_reports;
pub use velnor_actions_workflow_renderer::steps::FETCH_OPERATION as FETCH_OP;

pub fn retrieve_reports() -> Result<usize, OrchestratorError> {
    let run_id = std::env::var("GITHUB_RUN_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_run_id"))?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_id"))?;
    let run_key = velnor_actions_contract::resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&temp).join("velnor").join(&run_key);
    Ok(retrieve_reports_to(run_id, &run_dir))
}

pub(crate) fn retrieve_reports_to(run_id: u64, run_dir: &Path) -> usize {
    let plan = read_plan(run_dir);
    let Some(plan) = plan else {
        return 0;
    };
    let repo = std::env::var(crate::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .and_then(|raw| velnor_actions_contract::validate_repository_slug(&raw));
    let Some(repo) = repo else {
        return 0;
    };
    let catalog = ToolCatalog::pinned();
    let workspace = workspace_root();
    let mut retrieved = 0usize;
    for artifact_id in expected_artifact_ids_for_workspace(&plan, workspace.as_deref()) {
        if validate_artifact_id(&artifact_id).is_err() {
            continue;
        }
        let dir = run_dir.join("reports").join(&artifact_id);
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let Ok(args) = retrieve_args(run_id, &artifact_id, &dir, &repo) else {
            continue;
        };
        let (downloaded, _) = download_with_retry(|| {
            crate::gh_download::GhDownload::run(&catalog, run_dir, args.clone()).is_ok()
        });
        if downloaded {
            retrieved += 1;
        } else {
            drop(fs::remove_dir(&dir));
        }
    }
    retrieved
}

pub(crate) fn retrieve_args(
    run_id: u64,
    artifact_id: &str,
    dir: &Path,
    repo: &str,
) -> Result<Vec<OsString>, OrchestratorError> {
    validate_artifact_id(artifact_id).map_err(internal_contract)?;
    let Some(repo) = velnor_actions_contract::validate_repository_slug(repo) else {
        return Err(internal_contract(
            velnor_actions_contract::ContractError::identity("repository", "bad_lookup_repo"),
        ));
    };
    Ok(vec![
        OsString::from("run"),
        OsString::from("download"),
        OsString::from(run_id.to_string()),
        OsString::from("--name"),
        OsString::from(artifact_id),
        OsString::from("--dir"),
        dir.as_os_str().to_owned(),
        OsString::from("--repo"),
        OsString::from(repo),
    ])
}

pub(crate) const MAX_RETRIEVE_PLAN_BYTES: u64 = 4 << 20;

fn read_plan(run_dir: &Path) -> Option<serde_json::Value> {
    let text = read_staged_text(&run_dir.join("plan.json"), MAX_RETRIEVE_PLAN_BYTES).ok()?;
    parse_strict_json(&text).ok()
}

fn workspace_root() -> Option<PathBuf> {
    std::env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
}

fn expected_artifact_ids_for_workspace(
    plan: &serde_json::Value,
    workspace: Option<&Path>,
) -> Vec<String> {
    let ci_yml = workspace.and_then(|root| {
        read_staged_text(&root.join(".github/workflows/ci.yml"), MAX_RETRIEVE_PLAN_BYTES).ok()
    });
    let mut ids = Vec::new();
    for artifact_id in expected_artifact_ids(plan) {
        let hosted = format!(
            "{artifact_id}{}",
            velnor_actions_contract::workflow::lanes::HOSTED_SUFFIX
        );
        let local = format!(
            "{artifact_id}{}",
            velnor_actions_contract::workflow::lanes::SCALE_SUFFIX
        );
        if artifact_id.starts_with("velnor-crate-")
            && ci_yml
                .as_deref()
                .is_some_and(|text| text.contains(&hosted) && text.contains(&local))
        {
            ids.push(hosted);
            ids.push(local);
        } else {
            ids.push(artifact_id.to_owned());
        }
    }
    ids
}

fn expected_artifact_ids(plan: &serde_json::Value) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(entries) = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            if let Some(id) = entry.get("artifact_id").and_then(serde_json::Value::as_str)
                && seen.insert(id)
            {
                ids.push(id);
            }
        }
    }
    ids
}
