// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::CiError;
use crate::github::{
    ApiJob, ApiJobs, ApiRun, ApiStep, GitHub, Repo, job_log_path, jobs_path, parse, run_path,
};
use crate::logscan;
use crate::metrics::{JobMetrics, RunMetrics, StepMetrics};
use crate::time::{parse_utc, seconds_between};

pub fn collect(
    github: &dyn GitHub,
    repo: &Repo,
    run_id: u64,
    allow_rerun: bool,
) -> Result<RunMetrics, CiError> {
    let run: ApiRun = parse(&github.get(&run_path(repo, run_id))?, "run")?;
    if run.status != "completed" {
        return Err(CiError::new(format!(
            "run {run_id} is {}; measure a run only after it has completed",
            run.status
        )));
    }
    if run.conclusion.as_deref() != Some("success") {
        return Err(CiError::new(format!(
            "run {run_id} concluded {}; only successful runs are measured",
            run.conclusion.as_deref().unwrap_or("without a conclusion")
        )));
    }
    if run.run_attempt > 1 && !allow_rerun {
        return Err(CiError::new(format!(
            "run {run_id} is attempt {}; pass --allow-rerun to measure a re-run",
            run.run_attempt
        )));
    }
    let attempt_start = run
        .run_started_at
        .clone()
        .unwrap_or_else(|| run.created_at.clone());
    let mut jobs = Vec::new();
    for api_job in fetch_jobs(github, repo, run_id, run.run_attempt)? {
        jobs.push(job_metrics(github, repo, &api_job, &attempt_start)?);
    }
    let runner_seconds = jobs
        .iter()
        .filter(|job| job.ran())
        .map(|job| job.duration_seconds)
        .sum();
    Ok(RunMetrics {
        repo: repo.to_string(),
        run_id,
        run_attempt: run.run_attempt,
        workflow: run.name,
        event: run.event,
        head_sha: run.head_sha,
        wall_seconds: seconds_between(
            Some(run.run_started_at.as_deref().unwrap_or(&run.created_at)),
            Some(&run.updated_at),
        ),
        runner_seconds,
        jobs,
    })
}

const RESTORED_STEP: &str = "velnor-ci restored";
const PLAN_STEP: &str = "velnor-ci plan";

#[must_use]
pub fn setup_seconds(job: &ApiJob) -> Option<u64> {
    let started = job.started_at.as_deref()?;
    let ended_by = |name: &str| job.steps.iter().find(|step| step.name == name);
    let step = ended_by(RESTORED_STEP).or_else(|| ended_by(PLAN_STEP))?;
    let done = step.completed_at.as_deref()?;
    (step.conclusion.as_deref() == Some("success"))
        .then(|| seconds_between(Some(started), Some(done)))
}

fn fetch_jobs(
    github: &dyn GitHub,
    repo: &Repo,
    run_id: u64,
    attempt: u32,
) -> Result<Vec<ApiJob>, CiError> {
    let mut all = Vec::new();
    let mut page = 1;
    loop {
        let body = github.get(&jobs_path(repo, run_id, attempt, page))?;
        let ApiJobs { total_count, jobs } = parse(&body, "jobs")?;
        let empty = jobs.is_empty();
        all.extend(jobs);
        if empty || all.len() >= total_count {
            return Ok(all);
        }
        page += 1;
    }
}

fn predates(at: &str, attempt_start: &str) -> bool {
    matches!(
        (parse_utc(at), parse_utc(attempt_start)),
        (Some(at), Some(start)) if at < start
    )
}

fn job_metrics(
    github: &dyn GitHub,
    repo: &Repo,
    job: &ApiJob,
    attempt_start: &str,
) -> Result<JobMetrics, CiError> {
    let conclusion = job
        .conclusion
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let started = job
        .started_at
        .as_deref()
        .is_some_and(|at| !predates(at, attempt_start));
    let ran = conclusion != "skipped" && started;
    let (log, step_logs) = if ran {
        let text = github
            .get(&job_log_path(repo, job.id))
            .map_err(|e| e.context(&format!("log of job `{}`", job.name)))?;
        (logscan::scan(&text), step_logs(job, &text))
    } else {
        (logscan::LogScan::default(), BTreeMap::new())
    };
    let (queue_seconds, duration_seconds) = if ran {
        (
            seconds_between(Some(&job.created_at), job.started_at.as_deref()),
            seconds_between(job.started_at.as_deref(), job.completed_at.as_deref()),
        )
    } else {
        (0, 0)
    };
    let steps = job
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| StepMetrics {
            log: step_logs.get(&index).cloned().unwrap_or_default(),
            name: step.name.clone(),
            seconds: if step.conclusion.as_deref() == Some("skipped") {
                0
            } else {
                seconds_between(step.started_at.as_deref(), step.completed_at.as_deref())
            },
        })
        .collect();
    Ok(JobMetrics {
        name: job.name.clone(),
        labels: job.labels.clone(),
        conclusion,
        started,
        queue_seconds,
        duration_seconds,
        setup_seconds: if ran { setup_seconds(job) } else { None },
        steps,
        log,
    })
}

fn is_user_step(step: &ApiStep) -> bool {
    step.conclusion.as_deref() != Some("skipped")
        && step.name != "Set up job"
        && step.name != "Complete job"
        && !step.name.starts_with("Post ")
}

fn step_logs(job: &ApiJob, text: &str) -> BTreeMap<usize, logscan::LogScan> {
    let indexes: Vec<usize> = (0..job.steps.len())
        .filter(|index| is_user_step(&job.steps[*index]))
        .collect();
    let segments = logscan::split_steps(text);
    if segments.len() != indexes.len() {
        return BTreeMap::new();
    }
    indexes
        .into_iter()
        .zip(segments)
        .map(|(index, segment)| (index, logscan::scan(segment)))
        .collect()
}
