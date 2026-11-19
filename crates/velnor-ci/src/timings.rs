// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::Duration;

use velnor_model::{
    BUILD_TESTS_STEP, LINT_STEP, RepoPath, RepoView, RunnerLabel, TEST_STEP, Timings, WORKFLOW_PATH,
};
use velnor_rust::{TIMINGS_PATH, detect, root_job_ids};

use crate::collect::collect;
use crate::error::CiError;
use crate::github::{
    ApiListedRun, ApiPage, ApiRepo, GitHub, Repo, parse, repo_path, workflow_runs_path,
};
use crate::metrics::{JobMetrics, RunMetrics, StepMetrics};

pub const RUNS_NEEDED: usize = 3;
pub const RUNS_USED: usize = 10;
const PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimingsReport {
    pub timings: BTreeMap<RepoPath, Timings>,
    pub notes: Vec<String>,
}

#[derive(Debug)]
struct Sample {
    lint_build: u64,
    test_build: f64,
    tests: f64,
    binaries: BTreeMap<String, f64>,
}

pub fn current_setup(view: &RepoView) -> Result<(Vec<RepoPath>, RunnerLabel), CiError> {
    let mut without_timings = view.clone();
    without_timings.files.remove(&TIMINGS_PATH);
    let facts = detect(&without_timings).map_err(|e| CiError::new(e.to_string()))?;
    let roots = facts
        .rust_roots
        .iter()
        .map(|root| root.manifest.clone())
        .collect();
    Ok((roots, facts.config.runner))
}

pub fn gather(
    github: &dyn GitHub,
    repo: &Repo,
    roots: &[RepoPath],
    runner: &RunnerLabel,
) -> Result<TimingsReport, CiError> {
    let default_branch = default_branch(github, repo)?;
    let run_ids = ordinary_runs(github, repo, &default_branch)?;
    let mut notes = Vec::new();
    if run_ids.len() < RUNS_NEEDED {
        notes.push(format!(
            "{} has {} ordinary successful runs on {default_branch}; at least {RUNS_NEEDED} are needed, so no file is written",
            WORKFLOW_PATH.as_str(),
            run_ids.len()
        ));
        return Ok(TimingsReport {
            timings: BTreeMap::new(),
            notes,
        });
    }
    let mut runs = Vec::new();
    for id in &run_ids {
        match collect(github, repo, *id, false) {
            Ok(run) => runs.push(run),
            Err(error) if error.is_missing() => {
                notes.push(format!("run {id} has no readable logs and was skipped"));
            }
            Err(error) => return Err(error),
        }
    }
    if runs.len() < RUNS_NEEDED {
        notes.push(format!(
            "{} has {} runs with readable logs; at least {RUNS_NEEDED} are needed, so no file is written",
            WORKFLOW_PATH.as_str(),
            runs.len()
        ));
        return Ok(TimingsReport {
            timings: BTreeMap::new(),
            notes,
        });
    }
    let mut timings = BTreeMap::new();
    for root in roots {
        let samples: Vec<Sample> = runs
            .iter()
            .filter_map(|run| sample(run, root, roots.len() > 1, runner))
            .collect();
        if samples.len() < RUNS_NEEDED {
            notes.push(format!(
                "{root}: {} of {} runs on `{runner}` have its lint and test steps; at least {RUNS_NEEDED} are needed, so it has no entry",
                samples.len(),
                runs.len()
            ));
        } else {
            timings.insert(root.clone(), medians(&samples));
        }
    }
    Ok(TimingsReport { timings, notes })
}

fn default_branch(github: &dyn GitHub, repo: &Repo) -> Result<String, CiError> {
    let api: ApiRepo = parse(&github.get(&repo_path(repo))?, "repository")?;
    Ok(api.default_branch)
}

fn ordinary_runs(
    github: &dyn GitHub,
    repo: &Repo,
    default_branch: &str,
) -> Result<Vec<u64>, CiError> {
    let path = WORKFLOW_PATH;
    let workflow = path.as_str().rsplit('/').next().unwrap_or_default();
    let mut ids = Vec::new();
    for page in 1.. {
        let body = github.get(&workflow_runs_path(repo, workflow, page))?;
        let listed: ApiPage<ApiListedRun> = parse(&body, "workflow runs")?;
        let last_page = listed.items.len() < PAGE_SIZE;
        ids.extend(
            listed
                .items
                .iter()
                .filter(|run| is_ordinary(run, default_branch))
                .map(|run| run.id),
        );
        if last_page || ids.len() >= RUNS_USED {
            break;
        }
    }
    ids.truncate(RUNS_USED);
    Ok(ids)
}

fn is_ordinary(run: &ApiListedRun, default_branch: &str) -> bool {
    run.run_attempt == 1
        && match run.event.as_str() {
            "push" => run.head_branch.as_deref() == Some(default_branch),
            "pull_request" => run
                .pull_requests
                .iter()
                .any(|pull| pull.base.name == default_branch),
            _ => false,
        }
}

fn sample(
    run: &RunMetrics,
    root: &RepoPath,
    several_roots: bool,
    runner: &RunnerLabel,
) -> Option<Sample> {
    let [single, lint_job, test_job, group_job] = root_job_ids(root, several_roots);
    let step_in = |jobs: &[&str], step: &str| -> Option<&StepMetrics> {
        run.jobs
            .iter()
            .filter(|job| jobs.contains(&job.name.as_str()) && on_runner(job, runner))
            .find_map(|job| ran_step(job, step))
    };
    let lint = step_in(&[single.as_str(), lint_job.as_str()], LINT_STEP)?;
    let (build, test) = if let Some(test) = step_in(&[group_job.as_str()], TEST_STEP) {
        (step_in(&[test_job.as_str()], BUILD_TESTS_STEP)?, test)
    } else {
        let test = step_in(&[single.as_str(), test_job.as_str()], TEST_STEP)?;
        (test, test)
    };
    Some(Sample {
        lint_build: lint.seconds?,
        test_build: *build.log.finished_seconds.first()?,
        tests: test.log.nextest.first()?.seconds,
        binaries: test.log.binary_seconds.clone(),
    })
}

fn on_runner(job: &JobMetrics, runner: &RunnerLabel) -> bool {
    job.labels
        .iter()
        .any(|label| label.eq_ignore_ascii_case(runner.as_str()))
}

fn ran_step<'a>(job: &'a JobMetrics, name: &str) -> Option<&'a StepMetrics> {
    job.ran()
        .then(|| {
            job.steps
                .iter()
                .find(|step| step.name == name && step.seconds.is_some())
        })
        .flatten()
}

fn medians(samples: &[Sample]) -> Timings {
    let seconds = |pick: fn(&Sample) -> f64| whole(median(samples.iter().map(pick).collect()));
    let mut by_binary: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for sample in samples {
        for (binary, seconds) in &sample.binaries {
            by_binary.entry(binary).or_default().push(*seconds);
        }
    }
    Timings {
        lint_build_seconds: whole(median(
            samples.iter().map(|s| lossless(s.lint_build)).collect(),
        )),
        test_build_seconds: seconds(|s| s.test_build),
        test_seconds: seconds(|s| s.tests),
        test_binary_seconds: by_binary
            .into_iter()
            .filter(|(_, values)| values.len() >= RUNS_NEEDED)
            .map(|(binary, values)| (binary.to_owned(), whole(median(values))))
            .collect(),
    }
}

fn lossless(seconds: u64) -> f64 {
    Duration::from_secs(seconds).as_secs_f64()
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    match values.len() % 2 {
        1 => values[middle],
        _ => f64::midpoint(values[middle - 1], values[middle]),
    }
}

fn whole(seconds: f64) -> u64 {
    Duration::try_from_secs_f64(seconds.round()).map_or(0, |duration| duration.as_secs())
}
