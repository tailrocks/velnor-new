// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::WORKFLOW_PATH;

use crate::budget::{History, Population, Sample};
use crate::collect::{all_jobs, setup_seconds};
use crate::error::CiError;
use crate::github::{
    ApiJob, ApiListedRun, ApiPage, ApiRun, GitHub, Repo, event_runs_path, jobs_path, parse,
    run_jobs_path, run_path,
};
use crate::time::{parse_utc, seconds_between};

const PAGE_SIZE: usize = 100;
/// Runs come newest first, so a window older than this many pages of successful
/// runs is not a recent measurement.
const MAX_PAGES: u32 = 10;
const POPULATIONS: [Population; 3] = [
    Population::PullRequest,
    Population::MainPush,
    Population::Schedule,
];

/// The run that executes the budget job, measured up to `now`. It is judged by
/// its own figures: no earlier run says anything about it, and a finished run
/// that failed the budget never becomes the sample that fails the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    /// `None` for an event that has no history to compare with.
    pub population: Option<Population>,
    pub sample: Sample,
}

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
    running: Option<u64>,
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
                .filter(|run| run.run_attempt == 1 && run.succeeded() && Some(run.id) != running)
                .map(|run| run.id),
        );
        if ids.len() >= last || last_page {
            break;
        }
    }
    ids.truncate(last);
    Ok(ids)
}

/// The newest finished, successful runs of each population, newest first. The
/// running run, when it has a population, leads its window: it is judged by its
/// own figures against the runs before it, which never include a run that failed.
pub fn gather(
    github: &dyn GitHub,
    repo: &Repo,
    default_branch: &str,
    last: usize,
    running: Option<&Current>,
) -> Result<Vec<History>, CiError> {
    let mut histories = Vec::new();
    for population in POPULATIONS {
        let running = running.filter(|running| running.population == Some(population));
        let earlier = last.saturating_sub(usize::from(running.is_some()));
        let mut samples: Vec<Sample> = running
            .map(|running| running.sample.clone())
            .into_iter()
            .collect();
        let ids = newest_runs(
            github,
            repo,
            population,
            default_branch,
            earlier,
            running.map(|running| running.sample.run_id),
        )?;
        for run_id in ids {
            samples.push(sample(github, repo, run_id)?);
        }
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

fn population_of(event: &str) -> Option<Population> {
    match event {
        "pull_request" => Some(Population::PullRequest),
        "push" => Some(Population::MainPush),
        "schedule" => Some(Population::Schedule),
        _ => None,
    }
}

/// Measures the run `run_id` as it stands at `now` (seconds since the epoch). A
/// job that has not finished counts up to `now`, the way the run's wall time does.
pub fn current(
    github: &dyn GitHub,
    repo: &Repo,
    run_id: u64,
    now: i64,
) -> Result<Current, CiError> {
    let run: ApiRun = parse(&github.get(&run_path(repo, run_id))?, "run")?;
    let started = run.run_started_at.as_deref().unwrap_or(&run.created_at);
    let wall_seconds = parse_utc(started)
        .and_then(|start| u64::try_from(now - start).ok())
        .ok_or_else(|| CiError::new(format!("run {run_id} has no measurable wall time")))?;
    let mut runner_seconds = 0_u64;
    let mut setup: Option<u64> = None;
    let mut measured = Vec::new();
    for job in all_jobs(github, run_id, |page| run_jobs_path(repo, run_id, page))? {
        if job.conclusion.as_deref() == Some("skipped") {
            continue;
        }
        let Some(started) = job.started_at.as_deref() else {
            continue;
        };
        let end = job.completed_at.as_deref().map_or(Some(now), parse_utc);
        let duration = parse_utc(started)
            .zip(end)
            .and_then(|(start, end)| u64::try_from(end - start).ok())
            .ok_or_else(|| {
                CiError::new(format!(
                    "job {} of run {run_id} has no measurable duration",
                    job.name
                ))
            })?;
        runner_seconds = runner_seconds.saturating_add(duration);
        if let Some(seconds) = setup_seconds(&job) {
            setup = Some(setup.map_or(seconds, |current: u64| current.max(seconds)));
        }
        measured.push((job.name.clone(), duration));
    }
    Ok(Current {
        population: population_of(&run.event),
        sample: Sample {
            run_id,
            wall_seconds,
            runner_seconds,
            setup_seconds: setup,
            jobs: measured,
        },
    })
}
