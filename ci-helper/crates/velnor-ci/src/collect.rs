// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::github::{ApiJob, ApiJobs, ApiRun, GitHub, Repo, job_log_path, jobs_path, run_path};
use crate::logscan;
use crate::metrics::{JobMetrics, RunMetrics, StepMetrics};
use crate::time::seconds_between;

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
    if run.run_attempt > 1 && !allow_rerun {
        return Err(CiError::new(format!(
            "run {run_id} is attempt {}; pass --allow-rerun to measure a re-run",
            run.run_attempt
        )));
    }
    let mut jobs = Vec::new();
    for api_job in fetch_jobs(github, repo, run_id, run.run_attempt)? {
        jobs.push(job_metrics(github, repo, &api_job)?);
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

fn job_metrics(github: &dyn GitHub, repo: &Repo, job: &ApiJob) -> Result<JobMetrics, CiError> {
    let conclusion = job
        .conclusion
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let ran = conclusion != "skipped" && job.started_at.is_some();
    let log = if ran {
        let text = github
            .get(&job_log_path(repo, job.id))
            .map_err(|e| CiError::new(format!("log of job `{}`: {e}", job.name)))?;
        logscan::scan(&text)
    } else {
        logscan::LogScan::default()
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
        .map(|step| StepMetrics {
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
        conclusion,
        started: job.started_at.is_some(),
        queue_seconds,
        duration_seconds,
        steps,
        log,
    })
}

fn parse<T: serde::de::DeserializeOwned>(body: &str, what: &str) -> Result<T, CiError> {
    serde_json::from_str(body)
        .map_err(|e| CiError::new(format!("cannot read GitHub {what} response: {e}")))
}
