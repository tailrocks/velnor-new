// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use velnor_model::SpeedBudget;

use super::{api_url, current_dir, http, token};
use crate::budget;
use crate::budget_history::gather;
use crate::cli::BudgetArgs;
use crate::collect::all_jobs;
use crate::error::CiError;
use crate::github::{ApiRepo, ApiRun, GitHub, Repo, parse, repo_path, run_jobs_path, run_path};

const REPOSITORY_VARIABLE: &str = "GITHUB_REPOSITORY";
const SUMMARY_VARIABLE: &str = "GITHUB_STEP_SUMMARY";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Current {
    pub wall_seconds: u64,
    pub runner_seconds: u64,
}

pub(super) fn run_budget(args: &BudgetArgs) -> Result<String, CiError> {
    let root = current_dir()?;
    let Some(budget) = read_budget(&root)? else {
        return Ok("no speed budget is configured in .velnor/rust-ci.toml\n".to_owned());
    };
    let token = token()?;
    let repo = Repo::parse(&repository(args)?)?;
    let github = http::HttpGitHub::new(api_url(), token, http::DEFAULT_TIMEOUT);
    let default_branch = default_branch(&github, &repo)?;
    let histories = gather(&github, &repo, &default_branch, args.last)?;
    let evaluation = budget::evaluate(&budget, &histories);
    let report = budget::report(&budget, &evaluation);
    print!("{report}");
    write_summary(&report)?;
    write_current(&budget, &github, &repo)?;
    if evaluation.passed() {
        Ok(String::new())
    } else {
        Err(CiError::new(reason(&evaluation)))
    }
}

fn write_current(budget: &SpeedBudget, github: &dyn GitHub, repo: &Repo) -> Result<(), CiError> {
    let Ok(run_id) = std::env::var("GITHUB_RUN_ID") else {
        return Ok(());
    };
    let Ok(run_id) = run_id.parse::<u64>() else {
        return Ok(());
    };
    let measured = current(github, repo, run_id)?;
    write_current_report(budget, &measured)
}

fn read_budget(root: &Path) -> Result<Option<SpeedBudget>, CiError> {
    let path = root.join(velnor_rust::CONFIG_PATH.as_str());
    match fs::read(&path) {
        Ok(bytes) => velnor_rust::budget(&bytes).map_err(|error| CiError::new(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CiError::new(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

fn repository(args: &BudgetArgs) -> Result<String, CiError> {
    args.repo.clone().map_or_else(
        || {
            std::env::var(REPOSITORY_VARIABLE).map_err(|_| {
                CiError::new(format!(
                    "set {REPOSITORY_VARIABLE} or pass --repo owner/name to name the repository"
                ))
            })
        },
        Ok,
    )
}

fn default_branch(github: &dyn GitHub, repo: &Repo) -> Result<String, CiError> {
    let api: ApiRepo = parse(&github.get(&repo_path(repo))?, "repository")?;
    Ok(api.default_branch)
}

fn current(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<Current, CiError> {
    let run: ApiRun = parse(&github.get(&run_path(repo, run_id))?, "run")?;
    let now = now_seconds();
    let started = run.run_started_at.as_deref().unwrap_or(&run.created_at);
    let wall_seconds = crate::time::parse_utc(started)
        .and_then(|start| u64::try_from(now - start).ok())
        .ok_or_else(|| CiError::new(format!("run {run_id} has no measurable wall time")))?;
    let mut runner_seconds = 0_u64;
    for job in all_jobs(github, run_id, |page| run_jobs_path(repo, run_id, page))? {
        if job.conclusion.as_deref() == Some("skipped") {
            continue;
        }
        let Some(started) = job.started_at.as_deref() else {
            continue;
        };
        let end = match job.completed_at.as_deref() {
            Some(at) => crate::time::parse_utc(at),
            None => Some(now),
        };
        let duration = crate::time::parse_utc(started)
            .zip(end)
            .and_then(|(start, end)| u64::try_from(end - start).ok())
            .ok_or_else(|| {
                CiError::new(format!(
                    "job {} of run {run_id} has no measurable duration",
                    job.name
                ))
            })?;
        runner_seconds = runner_seconds.saturating_add(duration);
    }
    Ok(Current {
        wall_seconds,
        runner_seconds,
    })
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

fn reason(evaluation: &budget::Evaluation) -> String {
    let mut lines = Vec::new();
    for breach in &evaluation.breaches {
        lines.push(format!(
            "the {} {} of the {} is {}s, above the budget {}s",
            breach.population.name(),
            breach.measure.name(),
            breach.source.name(),
            breach.value,
            breach.threshold
        ));
    }
    for missing in &evaluation.missing {
        lines.push(missing.clone());
    }
    lines.join("\n")
}

fn write_summary(report: &str) -> Result<(), CiError> {
    super::append_to(SUMMARY_VARIABLE, report)
}

fn write_current_report(budget: &SpeedBudget, current: &Current) -> Result<(), CiError> {
    let report = budget::current_report(budget, current.wall_seconds, current.runner_seconds);
    print!("{report}");
    super::append_to(SUMMARY_VARIABLE, &report)
}
