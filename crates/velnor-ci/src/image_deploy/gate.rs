// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use serde::Deserialize;

use crate::error::CiError;
use crate::gate::{GateMode, release_gate_with, verify_exemptions};
use crate::github::{ApiRepo, GitHub, Repo, branch_path, parse, repo_path};

const NOT_YET: Duration = Duration::from_secs(90);
const LONGEST_PAUSE: Duration = Duration::from_secs(120);
const TRANSIENT_BUDGET: Duration = Duration::from_secs(60);
const TIP_PAUSE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub ready: bool,
    pub report: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Wait {
    pub total: Duration,
    pub interval: Duration,
}

impl std::fmt::Debug for Clock<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clock").finish_non_exhaustive()
    }
}

pub struct Clock<'a> {
    pub now: &'a dyn Fn() -> Duration,
    pub sleep: &'a dyn Fn(Duration),
}

#[derive(Debug)]
pub struct Request<'a> {
    pub sha: &'a str,
    pub require_tip: bool,
    pub wait: Wait,
    pub run_id: Option<u64>,
    pub pr_only_checks: &'a [String],
    pub verify_exemptions: bool,
    pub ignore_jobs: &'a [String],
    pub require_published: Option<&'a str>,
}

#[derive(Deserialize)]
struct Tip {
    commit: TipCommit,
}

#[derive(Deserialize)]
struct TipCommit {
    sha: String,
}

fn superseded(github: &dyn GitHub, repo: &Repo, sha: &str) -> Result<Option<String>, CiError> {
    let ApiRepo { default_branch, .. } = parse(&github.get(&repo_path(repo))?, "repository")?;
    let tip: Tip = parse(&github.get(&branch_path(repo, &default_branch))?, "branch")?;
    Ok((tip.commit.sha != sha).then(|| {
        format!(
            "commit {sha} is no longer the tip of {default_branch} ({}); a newer commit publishes instead\n",
            tip.commit.sha
        )
    }))
}

pub fn tip_only(
    github: &dyn GitHub,
    repo: &Repo,
    sha: &str,
    clock: &Clock<'_>,
) -> Result<Outcome, CiError> {
    let started = (clock.now)();
    loop {
        match superseded(github, repo, sha) {
            Ok(Some(report)) => {
                return Ok(Outcome {
                    ready: false,
                    report,
                });
            }
            Ok(None) => {
                return Ok(Outcome {
                    ready: true,
                    report: format!("commit {sha} is the tip of the default branch\n"),
                });
            }
            Err(error)
                if error.is_transient()
                    && (clock.now)().saturating_sub(started) < TRANSIENT_BUDGET =>
            {
                (clock.sleep)(TIP_PAUSE);
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn exemptions_only(
    github: &dyn GitHub,
    repo: &Repo,
    pr_only_checks: &[String],
    clock: &Clock<'_>,
) -> Result<Outcome, CiError> {
    let started = (clock.now)();
    loop {
        match verify_exemptions(github, repo, pr_only_checks) {
            Ok(report) => {
                return Ok(Outcome {
                    ready: true,
                    report,
                });
            }
            Err(error)
                if error.is_transient()
                    && (clock.now)().saturating_sub(started) < TRANSIENT_BUDGET =>
            {
                (clock.sleep)(TIP_PAUSE);
            }
            Err(error) => return Err(error),
        }
    }
}

enum Attempt {
    Ready(Outcome),
    Superseded(Outcome),
    Waiting(String),
}

fn attempt(github: &dyn GitHub, repo: &Repo, request: &Request<'_>) -> Result<Attempt, CiError> {
    let sha = request.sha;
    if request.require_tip
        && let Some(report) = superseded(github, repo, sha)?
    {
        return Ok(Attempt::Superseded(Outcome {
            ready: false,
            report,
        }));
    }
    let verdict = release_gate_with(
        github,
        repo,
        sha,
        request.pr_only_checks,
        request.run_id,
        GateMode {
            verify_exemptions: request.verify_exemptions,
            wait_for_unfinished: true,
            ignore_jobs: request.ignore_jobs,
            require_published: request.require_published,
        },
    )?;
    if !verdict.ready {
        return Ok(Attempt::Waiting(verdict.report));
    }
    if request.require_tip
        && let Some(report) = superseded(github, repo, sha)?
    {
        return Ok(Attempt::Superseded(Outcome {
            ready: false,
            report,
        }));
    }
    Ok(Attempt::Ready(Outcome {
        ready: true,
        report: verdict.report,
    }))
}

pub fn wait_for_required(
    github: &dyn GitHub,
    repo: &Repo,
    request: &Request<'_>,
    clock: &Clock<'_>,
) -> Result<Outcome, CiError> {
    let started = (clock.now)();
    let total = request.wait.total;
    let mut pause = request.wait.interval;
    loop {
        let result = attempt(github, repo, request);
        let elapsed = (clock.now)().saturating_sub(started);
        let transient = matches!(&result, Err(error) if error.is_transient());
        let limit = if transient {
            total.max(TRANSIENT_BUDGET)
        } else {
            total
        };
        match result {
            Ok(Attempt::Ready(outcome) | Attempt::Superseded(outcome)) => return Ok(outcome),
            Ok(Attempt::Waiting(report)) if elapsed >= total => {
                return Err(CiError::new(format!(
                    "the required checks of {} were not all green after {} seconds\n{report}",
                    request.sha,
                    total.as_secs(),
                )));
            }
            Err(error) if transient && elapsed >= limit => return Err(error),
            Err(error) if !transient && elapsed >= NOT_YET.min(total) => return Err(error),
            Ok(Attempt::Waiting(_)) | Err(_) => {}
        }
        (clock.sleep)(pause.min(limit.saturating_sub(elapsed)));
        pause = (pause + pause / 2).min(LONGEST_PAUSE);
    }
}
