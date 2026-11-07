// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionRead, OrchestratorError, Value, internal, require};

pub(super) fn verify_required_workflow(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
    suite: u64,
    branch: Option<&str>,
) -> Result<(), OrchestratorError> {
    let repository = branch.map(|_| api.get(prefix)).transpose()?;
    let mut found = false;
    let mut seen_runs = 0_u64;
    let mut expected_runs = None;
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/actions/runs?head_sha={source}&per_page=100&page={page}"
        ))?;
        let total = response["total_count"]
            .as_u64()
            .ok_or_else(|| internal("crate_release_runs_total"))?;
        require(total <= 10_000, "runs_limit")?;
        if let Some(previous) = expected_runs {
            require(previous == total, "runs_changed")?;
        }
        expected_runs = Some(total);
        let runs = response["workflow_runs"]
            .as_array()
            .ok_or_else(|| internal("crate_release_runs_invalid"))?;
        seen_runs = seen_runs
            .checked_add(
                u64::try_from(runs.len()).map_err(|_| internal("crate_release_runs_limit"))?,
            )
            .ok_or_else(|| internal("crate_release_runs_limit"))?;
        for run in runs {
            if run["check_suite_id"].as_u64() != Some(suite) {
                continue;
            }
            require(
                !found
                    && run["head_sha"].as_str() == Some(source)
                    && run["path"].as_str() == Some(workflow_path)
                    && run["status"].as_str() == Some("completed")
                    && run["conclusion"].as_str() == Some("success"),
                "required_workflow",
            )?;
            if let (Some(branch), Some(repository)) = (branch, &repository) {
                verify_release_origin(run, repository, prefix, branch)?;
            }
            found = true;
        }
        if seen_runs >= total {
            break;
        }
        require(!runs.is_empty(), "runs_pagination")?;
    }
    require(
        expected_runs == Some(seen_runs) && found,
        "required_workflow_missing",
    )?;
    Ok(())
}

fn verify_release_origin(
    run: &Value,
    repository: &Value,
    prefix: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    let name = prefix
        .strip_prefix("repos/")
        .ok_or_else(|| internal("crate_release_repository"))?;
    let owner = name
        .split_once('/')
        .ok_or_else(|| internal("crate_release_repository"))?
        .0;
    let id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_repository_id"))?;
    let owner_id = repository["owner"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_owner_id"))?;
    require(
        repository["full_name"].as_str() == Some(name)
            && repository["owner"]["login"].as_str() == Some(owner)
            && repository["default_branch"].as_str() == Some(branch)
            && run["event"].as_str() == Some("push")
            && run["head_branch"].as_str() == Some(branch),
        "required_workflow_origin",
    )?;
    for field in ["repository", "head_repository"] {
        require(
            run[field]["id"].as_u64() == Some(id)
                && run[field]["full_name"].as_str() == Some(name)
                && run[field]["owner"]["id"].as_u64() == Some(owner_id)
                && run[field]["owner"]["login"].as_str() == Some(owner),
            "required_workflow_repository",
        )?;
    }
    Ok(())
}
