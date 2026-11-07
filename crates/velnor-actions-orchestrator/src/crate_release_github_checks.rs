// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeSet, GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, Value, internal,
    require, required_workflow_suites, valid_sha, valid_utc, verify_reviews,
};

#[derive(Clone)]
struct CheckDecision {
    id: u64,
    suite: u64,
    status: String,
    conclusion: String,
    started: String,
}

pub(super) fn required_check(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
) -> Result<u64, OrchestratorError> {
    checked_required(api, prefix, source, workflow_path, None, None)
}

pub(super) fn release_required_check(
    api: &dyn GitHubAdmissionRead,
    request: &GitHubAdmissionRequest,
    source: &str,
    recorded: Option<u64>,
) -> Result<u64, OrchestratorError> {
    checked_required(
        api,
        &format!("repos/{}", request.repository),
        source,
        &request.required_workflow_path,
        Some(&request.default_branch),
        recorded,
    )
}

fn checked_required(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
    branch: Option<&str>,
    recorded_id: Option<u64>,
) -> Result<u64, OrchestratorError> {
    let workflow_suites = required_workflow_suites(api, prefix, source, workflow_path)?;
    let mut recorded = None;
    let mut latest: Option<CheckDecision> = None;
    let mut seen = 0_u64;
    let mut seen_ids = BTreeSet::new();
    let mut expected = None;
    for page in 1..=100 {
        let result: Value = api.get(&format!(
            "{prefix}/commits/{source}/check-runs?filter=all&per_page=100&page={page}"
        ))?;
        let total = result["total_count"]
            .as_u64()
            .ok_or_else(|| internal("crate_release_checks_total"))?;
        require(total <= 10_000, "checks_limit")?;
        if let Some(previous) = expected {
            require(previous == total, "checks_changed")?;
        }
        expected = Some(total);
        let checks = result["check_runs"]
            .as_array()
            .ok_or_else(|| internal("crate_release_checks_invalid"))?;
        seen = seen
            .checked_add(
                u64::try_from(checks.len()).map_err(|_| internal("crate_release_checks_limit"))?,
            )
            .ok_or_else(|| internal("crate_release_checks_limit"))?;
        for check in checks {
            let check_id = check["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("crate_release_check_id"))?;
            require(seen_ids.insert(check_id), "checks_duplicate")?;
            let Some(decision) = relevant_check(check, source, check_id, &workflow_suites)? else {
                continue;
            };
            remember_decision(&mut latest, &mut recorded, recorded_id, decision)?;
        }
        if seen >= total {
            break;
        }
        require(!checks.is_empty(), "checks_pagination")?;
    }
    require(expected == Some(seen), "checks_pagination")?;
    require(
        required_workflow_suites(api, prefix, source, workflow_path)? == workflow_suites,
        "runs_changed",
    )?;
    let latest = latest.ok_or_else(|| internal("crate_release_required_pending"))?;
    if latest.status != "completed" {
        return Err(internal("crate_release_required_pending"));
    }
    require(
        latest.status == "completed" && latest.conclusion == "success",
        "required_failed",
    )?;
    verify_required_workflow(api, prefix, source, workflow_path, latest.suite, branch)?;
    if let Some(id) = recorded_id {
        let recorded = recorded.ok_or_else(|| internal("crate_release_recorded_check_missing"))?;
        require(
            recorded.status == "completed" && recorded.conclusion == "success",
            "recorded_check_failed",
        )?;
        if recorded.suite != latest.suite {
            verify_required_workflow(api, prefix, source, workflow_path, recorded.suite, branch)?;
        }
        return Ok(id);
    }
    Ok(latest.id)
}

fn remember_decision(
    latest: &mut Option<CheckDecision>,
    recorded: &mut Option<CheckDecision>,
    recorded_id: Option<u64>,
    decision: CheckDecision,
) -> Result<(), OrchestratorError> {
    if recorded_id == Some(decision.id) {
        *recorded = Some(decision.clone());
    }
    if let Some(previous) = latest {
        if previous.started == decision.started {
            return require(previous.id == decision.id, "check_order_ambiguous");
        }
        if previous.started > decision.started {
            return Ok(());
        }
    }
    *latest = Some(decision);
    Ok(())
}

fn relevant_check(
    check: &Value,
    source: &str,
    id: u64,
    workflow_suites: &BTreeSet<u64>,
) -> Result<Option<CheckDecision>, OrchestratorError> {
    if check["name"].as_str() != Some("Required") {
        return Ok(None);
    }
    let suite = check["check_suite"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_check_suite"))?;
    if !workflow_suites.contains(&suite) {
        return Ok(None);
    }
    check_decision(check, source, id).map(Some)
}

fn check_decision(
    check: &Value,
    source: &str,
    id: u64,
) -> Result<CheckDecision, OrchestratorError> {
    require(check["head_sha"].as_str() == Some(source), "check_source")?;
    require(
        check["app"]["id"].as_u64() == Some(15368)
            && check["app"]["slug"].as_str() == Some("github-actions"),
        "check_app",
    )?;
    let suite = check["check_suite"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_check_suite"))?;
    let (status, started) = check_status_and_time(check)?;
    Ok(CheckDecision {
        id,
        suite,
        status: status.to_owned(),
        conclusion: check["conclusion"].as_str().unwrap_or_default().to_owned(),
        started: started.to_owned(),
    })
}

pub(super) fn check_status_and_time(check: &Value) -> Result<(&str, &str), OrchestratorError> {
    let status = check["status"]
        .as_str()
        .ok_or_else(|| internal("crate_release_check_status"))?;
    require(
        matches!(
            status,
            "queued" | "in_progress" | "waiting" | "requested" | "pending" | "completed"
        ),
        "check_status",
    )?;
    if status != "completed" && check["started_at"].is_null() {
        return Err(internal("crate_release_required_pending"));
    }
    let started = check["started_at"]
        .as_str()
        .filter(|value| valid_utc(value))
        .ok_or_else(|| internal("crate_release_check_time"))?;
    Ok((status, started))
}

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

pub(super) fn approved_pr(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    request: &GitHubAdmissionRequest,
    commit_sha: &str,
) -> Result<u64, OrchestratorError> {
    let mut candidates = Vec::new();
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/commits/{commit_sha}/pulls?per_page=100&page={page}"
        ))?;
        let prs = response
            .as_array()
            .ok_or_else(|| internal("crate_release_pulls_invalid"))?;
        let count = prs.len();
        for pr in prs {
            if pr["merged_at"].is_null() || pr["merge_commit_sha"].as_str() != Some(commit_sha) {
                continue;
            }
            if pr["base"]["ref"].as_str() != Some(request.default_branch.as_str())
                || pr["base"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
            {
                continue;
            }
            if pr["head"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
                || !pr["head"]["ref"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("release-plz-"))
            {
                continue;
            }
            candidates.push(pr.clone());
        }
        if count < 100 {
            break;
        }
        require(page < 100, "pulls_limit")?;
    }
    require(candidates.len() == 1, "release_pr")?;
    let pr = &candidates[0];
    let number = pr["number"]
        .as_u64()
        .ok_or_else(|| internal("crate_release_pr_number"))?;
    let author = pr["user"]["login"]
        .as_str()
        .ok_or_else(|| internal("crate_release_pr_author"))?;
    let author_id = pr["user"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_pr_author_id"))?;
    let author = super::reviews::ReviewAuthor {
        login: author,
        id: author_id,
    };
    let head = pr["head"]["sha"]
        .as_str()
        .ok_or_else(|| internal("crate_release_pr_head"))?;
    require(valid_sha(head), "release_pr_head")?;
    let merged = pr["merged_at"]
        .as_str()
        .filter(|value| valid_utc(value))
        .ok_or_else(|| internal("crate_release_merge_time"))?;
    let source_commit = api.get(&format!("{prefix}/git/commits/{commit_sha}"))?;
    let head_commit = api.get(&format!("{prefix}/git/commits/{head}"))?;
    let source_tree = source_commit["tree"]["sha"]
        .as_str()
        .filter(|s| valid_sha(s))
        .ok_or_else(|| internal("crate_release_source_tree"))?;
    require(
        source_commit["sha"].as_str() == Some(commit_sha)
            && head_commit["sha"].as_str() == Some(head)
            && head_commit["tree"]["sha"].as_str() == Some(source_tree),
        "reviewed_source_tree",
    )?;
    verify_reviews(api, prefix, number, author, head, merged)?;
    Ok(number)
}
