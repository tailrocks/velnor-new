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

mod exemptions;

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

struct PushEvidence {
    jobs: Vec<ApiRunJob>,
    names: BTreeSet<String>,
    running: bool,
}

struct RequiredChecks {
    names: BTreeSet<String>,
    exempt_sources: Vec<ApiRequiredCheck>,
}

fn refuse_blank_pr_only(pr_only_checks: &[String]) -> Result<(), CiError> {
    match pr_only_checks
        .iter()
        .position(|name| name.trim().is_empty())
    {
        Some(index) => Err(CiError::new(format!(
            "--pr-only-check #{} is blank; fix pr_only_checks in .velnor/release.toml",
            index + 1
        ))),
        None => Ok(()),
    }
}

pub fn release_gate(
    github: &dyn GitHub,
    repo: &Repo,
    sha: &str,
    pr_only_checks: &[String],
    current_run_id: Option<u64>,
) -> Result<Verdict, CiError> {
    refuse_blank_pr_only(pr_only_checks)?;
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
    let push = push_evidence(github, repo, sha, id, &default_branch)?;
    let statuses: Vec<ApiCommitStatus> = all_pages(github, "commit statuses", |page| {
        statuses_path(repo, sha, page)
    })?;
    let check_names =
        exemptions::check_names(github, repo, sha, &required.exempt_sources, current_run_id)?;
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
    for name in &required.names {
        if pr_only_checks.contains(name) {
            if push.names.contains(name)
                || check_names.contains(&name.to_lowercase())
                || statuses
                    .iter()
                    .any(|status| status.context.to_lowercase() == name.to_lowercase())
            {
                return Err(CiError::new(format!(
                    "PR-only check {name} has a push job or commit status or check run on commit {sha}"
                )));
            }
            report.push(format!("check {name} is explicitly PR-only"));
            continue;
        }
        let named: Vec<&str> = statuses
            .iter()
            .filter(|status| status.context.to_lowercase() == name.to_lowercase())
            .map(|status| status.state.as_str())
            .collect();
        match with_status(
            judge(name, &push.jobs, push.running, &default_branch),
            &named,
        ) {
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

fn push_evidence(
    github: &dyn GitHub,
    repo: &Repo,
    sha: &str,
    repository_id: u64,
    branch: &str,
) -> Result<PushEvidence, CiError> {
    let mut runs: Vec<ApiWorkflowRun> =
        all_pages(github, "workflow runs", |page| runs_path(repo, sha, page))?;
    runs.retain(|run| {
        run.event == "push"
            && run.head_sha == sha
            && run
                .head_repository
                .as_ref()
                .is_some_and(|head| head.id == repository_id)
    });
    let mut push = PushEvidence {
        jobs: Vec::new(),
        names: BTreeSet::new(),
        running: false,
    };
    for run in &runs {
        let jobs =
            all_pages::<ApiRunJob>(github, "jobs", |page| run_jobs_path(repo, run.id, page))?;
        push.names.extend(jobs.iter().map(|job| job.name.clone()));
        if run.head_branch.as_deref() == Some(branch) {
            push.jobs.extend(jobs);
            push.running |= run.status != "completed";
        }
    }
    Ok(push)
}

pub fn check_tag(version: &str, tag: &str) -> Result<String, CiError> {
    let legacy = format!("rust-ci-v{version}");
    let current = format!("tools-v{version}");
    if tag == legacy || tag == current {
        Ok(format!("tag {tag} names version {version}\n"))
    } else {
        Err(CiError::new(format!(
            "tag {tag} must be {legacy} or {current}, because this program is version {version}"
        )))
    }
}

fn required_checks(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
    pr_only_checks: &[String],
) -> Result<RequiredChecks, CiError> {
    let mut checks = BTreeSet::new();
    let mut app_checks = Vec::new();
    let mut applicable = Vec::new();
    for page in 1.. {
        let rules: Vec<ApiRule> = parse(
            &github.get(&rules_path(repo, branch, page))?,
            "branch rules",
        )?;
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
        let last = rules.len() < RULES_PAGE;
        applicable.extend(rules);
        if last {
            break;
        }
    }
    for check in classic_checks(github, repo, branch)? {
        checks.insert(check.context.clone());
        app_checks.push(check);
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
    if !pr_only_checks.is_empty() {
        exemptions::require_enforcement(
            github,
            repo,
            branch,
            &applicable,
            pr_only_checks,
            &app_checks,
        )?;
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
    Ok(RequiredChecks {
        names: checks,
        exempt_sources: app_checks
            .into_iter()
            .filter(|check| pr_only_checks.contains(&check.context))
            .collect(),
    })
}

fn classic_checks(
    github: &dyn GitHub,
    repo: &Repo,
    branch: &str,
) -> Result<Vec<ApiRequiredCheck>, CiError> {
    let ApiBranch { protection } = parse(&github.get(&branch_path(repo, branch))?, "branch")?;
    let classic = protection.required_status_checks;
    if classic.enforcement_level == "off" {
        return Ok(Vec::new());
    }
    let mut checks = classic.checks;
    for context in classic.contexts {
        if !checks.iter().any(|check| check.context == context) {
            checks.push(ApiRequiredCheck {
                context,
                integration_id: None,
            });
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
    pages(github, what, path, EmptyPage::Reject)
}

#[derive(Clone, Copy)]
enum EmptyPage {
    Reject,
    Discard,
}

fn pages<T: DeserializeOwned>(
    github: &dyn GitHub,
    what: &str,
    path: impl Fn(u32) -> String,
    empty_page: EmptyPage,
) -> Result<Vec<T>, CiError> {
    let mut all = Vec::new();
    let mut expected = None;
    for page in 1.. {
        let ApiPage { total_count, items } = parse(&github.get(&path(page))?, what)?;
        if expected.is_some_and(|count| count != total_count) {
            return Err(CiError::new(format!(
                "GitHub {what} listing changed during pagination; retry the release gate"
            )));
        }
        expected = Some(total_count);
        let empty = items.is_empty();
        all.extend(items);
        if all.len() == total_count {
            break;
        }
        if empty && matches!(empty_page, EmptyPage::Discard) {
            return Ok(Vec::new());
        }
        if empty || all.len() > total_count {
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
    let ours: Vec<&ApiRunJob> = jobs
        .iter()
        .filter(|job| job.name == name && job.conclusion.as_deref() != Some("skipped"))
        .collect();
    if ours.is_empty() {
        return if running {
            Check::Waiting("has not started".to_owned())
        } else if jobs.iter().any(|job| job.name == name) {
            Check::Failed("it concluded skipped".to_owned())
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
