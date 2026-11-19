// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::WORKFLOW_PATH;

use crate::budget::{History, Population, Sample};
use crate::collect::{all_jobs, setup_seconds};
use crate::error::CiError;
use crate::github::{
    ApiJob, ApiListedRun, ApiPage, ApiRun, GitHub, Repo, event_runs_path, jobs_path, parse,
    run_path,
};
use crate::time::seconds_between;

const PAGE_SIZE: usize = 100;
/// Runs come newest first, so a window older than this many pages of successful
/// runs is not a recent measurement.
const MAX_PAGES: u32 = 10;
const POPULATIONS: [Population; 3] = [
    Population::PullRequest,
    Population::MainPush,
    Population::Schedule,
];

fn listing(population: Population, default_branch: &str) -> (&'static str, Option<String>) {
    match population {
        Population::PullRequest => ("pull_request", None),
        Population::MainPush => ("push", Some(default_branch.to_owned())),
        Population::Schedule => ("schedule", None),
    }
}

fn newest_runs(
    github: &dyn GitHub,
    repo: &Repo,
    population: Population,
    default_branch: &str,
    last: usize,
) -> Result<Vec<u64>, CiError> {
    let path = WORKFLOW_PATH;
    let workflow = path.as_str().rsplit('/').next().unwrap_or_default();
    let (event, branch) = listing(population, default_branch);
    let mut ids = Vec::new();
    for page in 1..=MAX_PAGES {
        let body = github.get(&event_runs_path(
            repo,
            workflow,
            event,
            branch.as_deref(),
            page,
        ))?;
        let runs: ApiPage<ApiListedRun> = parse(&body, "workflow runs")?;
        let last_page = runs.items.len() < PAGE_SIZE;
        ids.extend(
            runs.items
                .into_iter()
                .filter(|run| run.run_attempt == 1 && run.succeeded())
                .map(|run| run.id),
        );
        if ids.len() >= last || last_page {
            break;
        }
    }
    ids.truncate(last);
    Ok(ids)
}

pub fn gather(
    github: &dyn GitHub,
    repo: &Repo,
    default_branch: &str,
    last: usize,
) -> Result<Vec<History>, CiError> {
    let mut histories = Vec::new();
    for population in POPULATIONS {
        let samples = newest_runs(github, repo, population, default_branch, last)?
            .into_iter()
            .map(|run_id| sample(github, repo, run_id))
            .collect::<Result<Vec<_>, _>>()?;
        if !samples.is_empty() {
            histories.push(History {
                population,
                samples,
            });
        }
    }
    Ok(histories)
}

fn job_seconds(job: &ApiJob, run_id: u64) -> Result<u64, CiError> {
    seconds_between(job.started_at.as_deref(), job.completed_at.as_deref()).ok_or_else(|| {
        CiError::new(format!(
            "job {} of run {run_id} has no measurable duration (started_at {}, completed_at {})",
            job.name,
            job.started_at.as_deref().unwrap_or("missing"),
            job.completed_at.as_deref().unwrap_or("missing")
        ))
    })
}

pub fn sample(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<Sample, CiError> {
    let run: ApiRun = parse(&github.get(&run_path(repo, run_id))?, "run")?;
    let jobs = all_jobs(github, run_id, |page| jobs_path(repo, run_id, 1, page))?;
    let mut runner_seconds = 0_u64;
    let mut setup: Option<u64> = None;
    let mut measured = Vec::new();
    for job in &jobs {
        if job.conclusion.as_deref() == Some("skipped") || job.started_at.is_none() {
            continue;
        }
        let duration = job_seconds(job, run_id)?;
        runner_seconds = runner_seconds.saturating_add(duration);
        if let Some(seconds) = setup_seconds(job) {
            setup = Some(setup.map_or(seconds, |current: u64| current.max(seconds)));
        }
        measured.push((job.name.clone(), duration));
    }
    let wall_seconds = seconds_between(
        Some(run.run_started_at.as_deref().unwrap_or(&run.created_at)),
        Some(&run.updated_at),
    )
    .ok_or_else(|| CiError::new(format!("run {run_id} has no measurable wall time")))?;
    Ok(Sample {
        run_id,
        wall_seconds,
        runner_seconds,
        setup_seconds: setup,
        jobs: measured,
    })
}
