// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use serde::Deserialize;

use crate::error::CiError;
use crate::gate::release_gate;
use crate::github::{ApiRepo, GitHub, Repo, branch_path, parse, repo_path};

const NOT_YET: Duration = Duration::from_secs(90);

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

pub fn wait_for_required(
    github: &dyn GitHub,
    repo: &Repo,
    sha: &str,
    require_tip: bool,
    wait: Wait,
    sleep: &dyn Fn(Duration),
    run_id: Option<u64>,
) -> Result<Outcome, CiError> {
    let mut elapsed = Duration::ZERO;
    loop {
        if require_tip && let Some(report) = superseded(github, repo, sha)? {
            return Ok(Outcome {
                ready: false,
                report,
            });
        }
        match release_gate(github, repo, sha, &[], run_id) {
            Ok(verdict) if verdict.ready => {
                return Ok(Outcome {
                    ready: true,
                    report: verdict.report,
                });
            }
            Ok(verdict) if elapsed >= wait.total => {
                return Err(CiError::new(format!(
                    "the required checks of {sha} were not all green after {} seconds\n{}",
                    wait.total.as_secs(),
                    verdict.report
                )));
            }
            Err(error) if elapsed >= NOT_YET.min(wait.total) => return Err(error),
            Ok(_) | Err(_) => {}
        }
        sleep(wait.interval);
        elapsed += wait.interval;
    }
}
