// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::error::CiError;
use crate::github::{
    ApiCommitStatus, ApiCompare, ApiRepo, ApiRunJob, ApiWorkflowRun, GitHub, Repo, compare_path,
    parse, repo_path, run_jobs_path, runs_path, statuses_path,
};

mod exemptions;
mod pages;
mod required;

use pages::{EmptyPage, all_pages, pages};
use required::required_checks;

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
