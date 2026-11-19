// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use velnor_model::{BUDGET_OUTPUTS, SpeedBudget, WORKFLOW_PATH};

use super::{api_url, current_dir, http, token};
use crate::budget::{self, History, Population};
use crate::cli::BudgetArgs;
use crate::collect::setup_seconds;
use crate::error::CiError;
use crate::github::{
    ApiJob, ApiJobs, ApiListedRun, ApiPage, ApiRepo, ApiRun, GitHub, Repo, jobs_path, parse,
    repo_path, run_jobs_path, run_path, workflow_runs_path,
};
use crate::time::seconds_between;

const PAGE_SIZE: usize = 100;
/// Runs come newest first, so a window older than this many pages of successful
/// runs is not a recent measurement.
const MAX_PAGES: u32 = 10;
const REPOSITORY_VARIABLE: &str = "GITHUB_REPOSITORY";
const SUMMARY_VARIABLE: &str = "GITHUB_STEP_SUMMARY";
const OUTPUT_VARIABLE: &str = "GITHUB_OUTPUT";

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
    write_outputs(budget, &measured)
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

fn classify(run: &ApiListedRun, default_branch: &str) -> Option<Population> {
    match run.event.as_str() {
        "pull_request" => Some(Population::PullRequest),
        "push" if run.head_branch.as_deref() == Some(default_branch) => Some(Population::MainPush),
        "schedule" => Some(Population::Schedule),
        _ => None,
    }
}

fn gather(
    github: &dyn GitHub,
    repo: &Repo,
    default_branch: &str,
    last: usize,
) -> Result<Vec<History>, CiError> {
    let path = WORKFLOW_PATH;
    let workflow = path.as_str().rsplit('/').next().unwrap_or_default();
    let mut listed: Vec<(Population, u64)> = Vec::new();
    let mut counts: BTreeMap<Population, usize> = BTreeMap::new();
    for page in 1..=MAX_PAGES {
        let body = github.get(&workflow_runs_path(repo, workflow, page))?;
        let runs: ApiPage<ApiListedRun> = parse(&body, "workflow runs")?;
        let last_page = runs.items.len() < PAGE_SIZE;
        for run in runs.items {
            if run.run_attempt != 1 || !run.succeeded() {
                continue;
            }
            if let Some(population) = classify(&run, default_branch) {
                *counts.entry(population).or_default() += 1;
                listed.push((population, run.id));
            }
        }
        let enough = [
            Population::PullRequest,
            Population::MainPush,
            Population::Schedule,
        ]
        .iter()
        .all(|population| counts.get(population).is_some_and(|count| *count >= last));
        if last_page || enough {
            break;
        }
    }
    let mut samples: BTreeMap<Population, Vec<budget::Sample>> = BTreeMap::new();
    for (population, run_id) in listed {
        let entry = samples.entry(population).or_default();
        if entry.len() >= last {
            continue;
        }
        entry.push(sample(github, repo, run_id)?);
    }
    Ok(samples
        .into_iter()
        .map(|(population, samples)| History {
            population,
            samples,
        })
        .collect())
}

fn sample(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<budget::Sample, CiError> {
    let run: ApiRun = parse(&github.get(&run_path(repo, run_id))?, "run")?;
    let jobs = fetch_jobs(github, repo, run_id)?;
    let mut runner_seconds = 0_u64;
    let mut setup: Option<u64> = None;
    let mut measured = Vec::new();
    for job in &jobs {
        if job.conclusion.as_deref() == Some("skipped") || job.started_at.is_none() {
            continue;
        }
        let duration =
            seconds_between(job.started_at.as_deref(), job.completed_at.as_deref()).unwrap_or(0);
        runner_seconds = runner_seconds.saturating_add(duration);
        if let Some(seconds) = setup_seconds(job) {
            setup = Some(setup.map_or(seconds, |current: u64| current.max(seconds)));
        }
        measured.push((job.name.clone(), duration));
    }
    Ok(budget::Sample {
        run_id,
        wall_seconds: seconds_between(
            Some(run.run_started_at.as_deref().unwrap_or(&run.created_at)),
            Some(&run.updated_at),
        )
        .unwrap_or(0),
        runner_seconds,
        setup_seconds: setup,
        jobs: measured,
    })
}

fn fetch_jobs(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<Vec<ApiJob>, CiError> {
    let mut all = Vec::new();
    for page in 1_u32.. {
        let body = github.get(&jobs_path(repo, run_id, 1, page))?;
        let jobs: ApiJobs = parse(&body, "jobs")?;
        let empty = jobs.jobs.is_empty();
        let total = jobs.total_count;
        all.extend(jobs.jobs);
        if empty || all.len() >= total {
            return Ok(all);
        }
    }
    Ok(all)
}

fn current(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<Current, CiError> {
    let run: ApiRun = parse(&github.get(&run_path(repo, run_id))?, "run")?;
    let now = now_seconds();
    let started = run
        .run_started_at
        .as_deref()
        .or(Some(run.created_at.as_str()));
    let wall_seconds = crate::time::parse_utc(started.unwrap_or_default())
        .and_then(|start| u64::try_from(now - start).ok())
        .unwrap_or(0);
    let mut runner_seconds = 0_u64;
    for job in fetch_run_jobs(github, repo, run_id)? {
        if job.conclusion.as_deref() == Some("skipped") {
            continue;
        }
        let Some(started) = job.started_at.as_deref() else {
            continue;
        };
        let end = job
            .completed_at
            .as_deref()
            .map_or(now, |at| crate::time::parse_utc(at).unwrap_or(now));
        let Some(start) = crate::time::parse_utc(started) else {
            continue;
        };
        let duration = u64::try_from(end - start).unwrap_or(0);
        runner_seconds = runner_seconds.saturating_add(duration);
    }
    Ok(Current {
        wall_seconds,
        runner_seconds,
    })
}

fn fetch_run_jobs(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<Vec<ApiJob>, CiError> {
    let mut all = Vec::new();
    for page in 1_u32.. {
        let body = github.get(&run_jobs_path(repo, run_id, page))?;
        let jobs: ApiJobs = parse(&body, "jobs")?;
        let empty = jobs.jobs.is_empty();
        let total = jobs.total_count;
        all.extend(jobs.jobs);
        if empty || all.len() >= total {
            return Ok(all);
        }
    }
    Ok(all)
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

fn write_outputs(budget: &SpeedBudget, current: &Current) -> Result<(), CiError> {
    let values = [
        budget.pr_wall_seconds.to_string(),
        budget.main_wall_seconds.to_string(),
        budget.runner_seconds.to_string(),
        budget.setup_seconds.to_string(),
        current.wall_seconds.to_string(),
        current.runner_seconds.to_string(),
    ];
    let outputs: Vec<String> = BUDGET_OUTPUTS
        .iter()
        .zip(values)
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    super::append_to(OUTPUT_VARIABLE, &format!("{}\n", outputs.join("\n")))
}
