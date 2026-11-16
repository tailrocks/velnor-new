// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;

use crate::error::CiError;
use crate::github::{
    ApiBranch, ApiCommitStatus, ApiCompare, ApiPage, ApiRepo, ApiRequiredCheck, ApiRule, ApiRunJob,
    ApiWorkflowRun, GitHub, Repo, branch_path, compare_path, parse, repo_path, rules_path,
    run_jobs_path, runs_path, statuses_path,
};

const ACTIONS_APP_ID: i64 = 15368;
const ANY_APP: i64 = -1;
const RULES_PAGE: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub ready: bool,
    pub report: String,
}

enum Check {
    Passed,
    Waiting(String),
    Failed(String),
}

pub fn release_gate(
    github: &dyn GitHub,
    repo: &Repo,
    sha: &str,
    pr_only_checks: &[String],
) -> Result<Verdict, CiError> {
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(CiError::new(format!(
            "`{sha}` is not a full lowercase commit SHA"
        )));
    }
    let ApiRepo { id, default_branch } = parse(&github.get(&repo_path(repo))?, "repository")?;
    let required = required_checks(github, repo, &default_branch, pr_only_checks)?;
    let compare: ApiCompare = parse(
        &github.get(&compare_path(repo, &default_branch, sha))?,
        "comparison",
    )?;
    let mut runs: Vec<ApiWorkflowRun> =
        all_pages(github, "workflow runs", |page| runs_path(repo, sha, page))?;
    runs.retain(|run| {
        run.event == "push"
            && run.head_branch.as_deref() == Some(default_branch.as_str())
            && run.head_sha == sha
            && run
                .head_repository
                .as_ref()
                .is_some_and(|head| head.id == id)
    });
    let mut jobs = Vec::new();
    for run in &runs {
        jobs.extend(all_pages::<ApiRunJob>(github, "jobs", |page| {
            run_jobs_path(repo, run.id, page)
        })?);
    }
    let running = runs.iter().any(|run| run.status != "completed");
    let statuses: Vec<ApiCommitStatus> = all_pages(github, "commit statuses", |page| {
        statuses_path(repo, sha, page)
    })?;
    let mut report = Vec::new();
    let mut failures = Vec::new();
    if matches!(compare.status.as_str(), "identical" | "behind") {
        report.push(format!("commit {sha} is on {default_branch}"));
    } else {
        failures.push(format!(
            "commit {sha} is not on the default branch {default_branch} (comparison: {})",
            compare.status
        ));
    }
    let mut ready = true;
    for name in &required {
        if pr_only_checks.contains(name) {
            report.push(format!("check {name} is explicitly PR-only"));
            continue;
        }
        let named: Vec<&str> = statuses
            .iter()
            .filter(|status| status.context.to_lowercase() == name.to_lowercase())
            .map(|status| status.state.as_str())
            .collect();
        match with_status(judge(name, &jobs, running, &default_branch), &named) {
            Check::Passed => report.push(format!("check {name} passed")),
            Check::Waiting(state) => {
                ready = false;
                report.push(format!("check {name} {state}"));
            }
            Check::Failed(reason) => failures.push(format!(
                "check {name} did not pass on commit {sha}: {reason}"
            )),
        }
    }
    if !failures.is_empty() {
        return Err(CiError::new(failures.join("\n")));
    }
    report.push(format!("ready={ready}"));
    Ok(Verdict {
        ready,
        report: report.join("\n") + "\n",
    })
}

pub fn check_tag(version: &str, tag: &str) -> Result<String, CiError> {
    let expected = format!("rust-ci-v{version}");
    if tag == expected {
        Ok(format!("tag {tag} names version {version}\n"))
    } else {
        Err(CiError::new(format!(
            "tag {tag} must be {expected}, because this program is version {version}"
        )))
    }
}

fn required_checks(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
    pr_only_checks: &[String],
) -> Result<BTreeSet<String>, CiError> {
    let mut checks = BTreeSet::new();
    let mut app_checks = Vec::new();
    let mut pull_request = false;
    for page in 1.. {
        let rules: Vec<ApiRule> = parse(
            &github.get(&rules_path(repo, branch, page))?,
            "branch rules",
        )?;
        pull_request |= rules.iter().any(|rule| rule.kind == "pull_request");
        for rule in rules
            .iter()
            .filter(|rule| rule.kind == "required_status_checks")
        {
            let listed = rule
                .parameters
                .as_ref()
                .and_then(|parameters| parameters.required_status_checks.as_ref())
                .ok_or_else(|| {
                    CiError::new(format!(
                        "a required_status_checks rule of branch {branch} lists no checks"
                    ))
                })?;
            for check in listed {
                checks.insert(check.context.clone());
                app_checks.push(check.clone());
            }
        }
        if rules.len() < RULES_PAGE {
            break;
        }
    }
    let ApiBranch { protection } = parse(&github.get(&branch_path(repo, branch))?, "branch")?;
    let classic = protection.required_status_checks;
    if classic.enforcement_level != "off" {
        checks.extend(classic.contexts);
        for check in &classic.checks {
            checks.insert(check.context.clone());
            app_checks.push(check.clone());
        }
    }
    if checks.is_empty() {
        return Err(CiError::new(format!(
            "branch {branch} requires no status check in its rules or its branch protection, so no commit of it can be released"
        )));
    }
    for name in pr_only_checks {
        if !checks.contains(name) {
            return Err(CiError::new(format!(
                "PR-only check {name} is not required on branch {branch}"
            )));
        }
    }
    if !pr_only_checks.is_empty() && !pull_request {
        return Err(CiError::new(format!(
            "branch {branch} has no pull_request rule, so its required checks cannot be exempted as PR-only"
        )));
    }
    if checks.iter().all(|name| pr_only_checks.contains(name)) {
        return Err(CiError::new(format!(
            "branch {branch} has no required push check after PR-only exemptions"
        )));
    }
    for check in &app_checks {
        if !pr_only_checks.contains(&check.context) {
            require(branch, check)?;
        }
    }
    Ok(checks)
}

fn require(branch: &str, check: &ApiRequiredCheck) -> Result<(), CiError> {
    if let Some(app) = check
        .integration_id
        .filter(|app| ![ACTIONS_APP_ID, ANY_APP].contains(app))
    {
        return Err(CiError::new(format!(
            "branch {branch} requires check {} from the GitHub App {app}, and only a check from GitHub Actions ({ACTIONS_APP_ID}) can be tied to a push run",
            check.context
        )));
    }
    Ok(())
}

fn all_pages<T: DeserializeOwned>(
    github: &dyn GitHub,
    what: &str,
    path: impl Fn(u32) -> String,
) -> Result<Vec<T>, CiError> {
    let mut all = Vec::new();
    for page in 1.. {
        let ApiPage { total_count, items } = parse(&github.get(&path(page))?, what)?;
        let empty = items.is_empty();
        all.extend(items);
        if all.len() >= total_count {
            break;
        }
        if empty {
            return Err(CiError::new(format!(
                "GitHub lists {total_count} {what} and returned {}",
                all.len()
            )));
        }
    }
    Ok(all)
}

fn with_status(by_jobs: Check, states: &[&str]) -> Check {
    if let Check::Failed(reason) = by_jobs {
        return Check::Failed(reason);
    }
    if let Some(state) = states
        .iter()
        .find(|state| !matches!(**state, "success" | "pending"))
    {
        return Check::Failed(format!("its commit status is {state}"));
    }
    if states.contains(&"pending") {
        return Check::Waiting("has a pending commit status".to_owned());
    }
    by_jobs
}

fn judge(name: &str, jobs: &[ApiRunJob], running: bool, branch: &str) -> Check {
    let ours: Vec<&ApiRunJob> = jobs.iter().filter(|job| job.name == name).collect();
    if ours.is_empty() {
        return if running {
            Check::Waiting("has not started".to_owned())
        } else {
            Check::Failed(format!("no job of a push run on {branch} reports it"))
        };
    }
    if let Some(job) = ours
        .iter()
        .find(|job| job.status == "completed" && job.conclusion.as_deref() != Some("success"))
    {
        return Check::Failed(format!(
            "it concluded {}",
            job.conclusion.as_deref().unwrap_or("without a result")
        ));
    }
    match ours.iter().find(|job| job.status != "completed") {
        Some(job) => Check::Waiting(format!("is {}", job.status)),
        None => Check::Passed,
    }
}
