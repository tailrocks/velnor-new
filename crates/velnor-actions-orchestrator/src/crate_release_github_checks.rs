// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, BTreeSet, GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, Value,
    internal, require, required_workflow_suites, valid_utc,
    workflow_checks::verify_required_workflow,
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
    let (latest_suite, latest_job) = latest_required_job(api, prefix, &workflow_suites, branch)?;
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
            if check_id != latest_job && recorded_id != Some(check_id) {
                continue;
            }
            let Some(decision) = relevant_check(check, source, check_id, &workflow_suites)? else {
                continue;
            };
            if recorded_id == Some(decision.id) {
                recorded = Some(decision.clone());
            }
            if decision.suite == latest_suite && decision.id == latest_job {
                remember_decision(&mut latest, &mut None, None, decision)?;
            }
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
    let selected_id = selected_check(
        api,
        prefix,
        source,
        workflow_path,
        branch,
        (recorded_id, recorded),
        &latest,
    )?;
    require(
        required_workflow_suites(api, prefix, source, workflow_path)? == workflow_suites,
        "runs_changed",
    )?;
    Ok(selected_id)
}

fn selected_check(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    source: &str,
    workflow_path: &str,
    branch: Option<&str>,
    recorded: (Option<u64>, Option<CheckDecision>),
    latest: &CheckDecision,
) -> Result<u64, OrchestratorError> {
    if let Some(id) = recorded.0 {
        let recorded = recorded.1;
        let recorded = recorded.ok_or_else(|| internal("crate_release_recorded_check_missing"))?;
        require(
            recorded.status == "completed" && recorded.conclusion == "success",
            "recorded_check_failed",
        )?;
        if recorded.suite != latest.suite {
            verify_required_workflow(api, prefix, source, workflow_path, recorded.suite, branch)?;
        }
        Ok(id)
    } else {
        Ok(latest.id)
    }
}

fn latest_required_job(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    suites: &BTreeMap<u64, Value>,
    branch: Option<&str>,
) -> Result<(u64, u64), OrchestratorError> {
    let mut latest: Option<(u64, u64, u64, u64, &Value)> = None;
    for (&suite, run) in suites {
        if let Some(branch) = branch
            && (run["event"].as_str() != Some("push")
                || run["head_branch"].as_str() != Some(branch))
        {
            continue;
        }
        let number = run["run_number"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| internal("crate_release_run_number"))?;
        let attempt = run["run_attempt"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| internal("crate_release_run_attempt"))?;
        let run_id = run["id"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| internal("crate_release_run_id"))?;
        if let Some((previous, _, _, _, _)) = latest {
            require(previous != number, "run_order_ambiguous")?;
            if previous > number {
                continue;
            }
        }
        latest = Some((number, attempt, run_id, suite, run));
    }
    let (_, attempt, run_id, suite, run) =
        latest.ok_or_else(|| internal("crate_release_required_pending"))?;
    if run["status"].as_str() != Some("completed") {
        return Err(internal("crate_release_required_pending"));
    }
    require(
        run["conclusion"].as_str() == Some("success"),
        "required_failed",
    )?;
    Ok((suite, required_job(api, prefix, run_id, attempt)?))
}

fn required_job(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    run_id: u64,
    attempt: u64,
) -> Result<u64, OrchestratorError> {
    let mut required = None;
    let mut seen = 0_u64;
    let mut expected = None;
    for page in 1..=100 {
        let result = api.get(&format!(
            "{prefix}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100&page={page}"
        ))?;
        let total = result["total_count"]
            .as_u64()
            .ok_or_else(|| internal("crate_release_jobs_total"))?;
        require(total <= 10_000, "jobs_limit")?;
        if let Some(previous) = expected {
            require(previous == total, "jobs_changed")?;
        }
        expected = Some(total);
        let jobs = result["jobs"]
            .as_array()
            .ok_or_else(|| internal("crate_release_jobs_invalid"))?;
        seen = seen
            .checked_add(
                u64::try_from(jobs.len()).map_err(|_| internal("crate_release_jobs_limit"))?,
            )
            .ok_or_else(|| internal("crate_release_jobs_limit"))?;
        for job in jobs {
            if job["name"].as_str() != Some("Required") {
                continue;
            }
            let id = job["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("crate_release_job_id"))?;
            require(
                required.is_none()
                    && job["run_id"].as_u64() == Some(run_id)
                    && job["run_attempt"].as_u64() == Some(attempt)
                    && job["check_run_url"].as_str()
                        == Some(&format!("https://api.github.com/{prefix}/check-runs/{id}")),
                "required_job_identity",
            )?;
            require(
                job["status"].as_str() == Some("completed")
                    && job["conclusion"].as_str() == Some("success"),
                "required_failed",
            )?;
            required = Some(id);
        }
        if seen >= total {
            break;
        }
        require(!jobs.is_empty(), "jobs_pagination")?;
    }
    require(expected == Some(seen), "jobs_pagination")?;
    required.ok_or_else(|| internal("crate_release_required_job_missing"))
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
    workflow_suites: &BTreeMap<u64, Value>,
) -> Result<Option<CheckDecision>, OrchestratorError> {
    if check["name"].as_str() != Some("Required") {
        return Ok(None);
    }
    let suite = check["check_suite"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_check_suite"))?;
    if !workflow_suites.contains_key(&suite) {
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
