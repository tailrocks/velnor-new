// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cli::ReportArgs;
use crate::error::CiError;
use crate::github::{ApiJobs, GitHub, Repo, jobs_path};

use crate::report::{LaneMetrics, SCHEMA, SaveOutcome, check, waste};
use crate::report_caches::{CachePlan, observe_caches};
use crate::telemetry::{INVOCATIONS, Invocation};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}
fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn temp(file: &str) -> PathBuf {
    Path::new(&env("RUNNER_TEMP")).join(file)
}
fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, CiError> {
    serde_json::from_str(text).map_err(ci)
}

fn invocations() -> (Vec<Invocation>, Vec<String>) {
    let path = temp(INVOCATIONS);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), Vec::new());
        }
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("cannot read {}: {error}", path.display())],
            );
        }
    };
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice(line) {
            Ok(invocation) => records.push(invocation),
            Err(error) => errors.push(format!(
                "{} line {} is invalid: {error}",
                INVOCATIONS,
                index + 1
            )),
        }
    }
    (records, errors)
}

fn read_log() -> Result<Vec<u8>, CiError> {
    match fs::read(temp("velnor-run.log")) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(ci(error)),
    }
}

pub(super) fn run_report(args: &ReportArgs) -> Result<String, CiError> {
    if let Some(directory) = &args.check {
        return check_directory(directory);
    }
    let log_bytes = read_log()?;
    let (invocations, mut invocation_errors) = invocations();
    let plans: Vec<CachePlan> =
        json(&std::env::var("VELNOR_REPORT_CACHES").unwrap_or_else(|_| "[]".to_owned()))?;
    let steps: Value =
        json(&std::env::var("VELNOR_REPORT_STEPS").unwrap_or_else(|_| "{}".to_owned()))?;
    let (caches, cache_errors) = observe_caches(
        plans,
        &steps,
        env("GITHUB_REF") == format!("refs/heads/{}", env("VELNOR_REPORT_DEFAULT_BRANCH")),
    );
    invocation_errors.extend(cache_errors);
    let mut metrics = LaneMetrics {
        schema: SCHEMA,
        lane: env("GITHUB_JOB"),
        job_name: std::env::var("VELNOR_REPORT_JOB_NAME").ok(),
        caches,
        invocations,
        downloaded: 0,
        first_cargo_seconds: None,
        slowest_tests: Vec::new(),
        tests_run: env("VELNOR_TESTS_RUN").parse().ok(),
        diagnostics: Vec::new(),
        verification_errors: invocation_errors,
        violations: Vec::new(),
    };
    metrics.invocations.retain(|invocation| {
        invocation.argv.first().is_some_and(|program| {
            Path::new(program)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("cargo"))
        })
    });
    if !metrics.invocations.is_empty()
        && let Err(error) = super::report_inputs::collect_inputs(&mut metrics, &log_bytes)
    {
        metrics
            .verification_errors
            .push(format!("Restored input provenance unavailable: {error}"));
    }
    if let Err(error) = super::report_inputs::verify_downloads(&mut metrics, &log_bytes) {
        metrics
            .verification_errors
            .push(format!("Download provenance unavailable: {error}"));
    }
    activity_metrics(&mut metrics, &log_bytes);
    metrics.violations = waste(&metrics);
    let summary = metrics.summary();
    fs::write(
        temp("ci-metrics.json"),
        serde_json::to_string_pretty(&metrics)
            .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?,
    )
    .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?;
    super::append_to("GITHUB_STEP_SUMMARY", &summary)?;
    Ok(summary)
}

fn api_metrics(metrics: &mut LaneMetrics) -> Result<(), CiError> {
    let repo = Repo::parse(&env("GITHUB_REPOSITORY"))?;
    let github = super::http::HttpGitHub::new(
        super::api_url(),
        super::token()?,
        super::http::DEFAULT_TIMEOUT,
    );
    let run = env("GITHUB_RUN_ID")
        .parse()
        .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?;
    let attempt = env("GITHUB_RUN_ATTEMPT")
        .parse()
        .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?;
    for page in 1.. {
        let jobs: ApiJobs = json(&github.get(&jobs_path(&repo, run, attempt, page))?)?;
        if let Some(job) = jobs
            .jobs
            .iter()
            .find(|job| job.name == metrics.job_name.as_deref().unwrap_or(&metrics.lane))
        {
            metrics.first_cargo_seconds = job
                .started_at
                .as_deref()
                .and_then(crate::time::parse_utc)
                .and_then(|start| {
                    metrics
                        .invocations
                        .iter()
                        .map(|invocation| invocation.started_at)
                        .min()
                        .and_then(|first| first.checked_sub(u64::try_from(start).ok()?))
                });
        }
        if jobs.jobs.len() < 100 {
            break;
        }
    }
    if metrics.first_cargo_seconds.is_none() {
        metrics
            .diagnostics
            .push("Job start to first cargo is unavailable".to_owned());
    }
    cache_metrics(metrics, &github, &repo)
}

fn cache_metrics(
    metrics: &mut LaneMetrics,
    github: &impl GitHub,
    repo: &Repo,
) -> Result<(), CiError> {
    for cache in &mut metrics.caches {
        let saving = cache.save != SaveOutcome::NotAttempted;
        let key = if saving {
            &cache.requested_key
        } else if let Some(key) = &cache.matched_key {
            key
        } else {
            continue;
        };
        if key.is_empty() {
            cache.bytes = None;
            metrics
                .diagnostics
                .push("Cache inventory key unavailable".to_owned());
            continue;
        }
        let listing: Value = json(&github.get(&format!(
            "/repos/{repo}/actions/caches?key={}&per_page=100",
            crate::cache::encode(key)
        ))?)?;
        let entry = match inventory_entry(&listing, key, saving) {
            Ok(entry) => entry,
            Err(error) => {
                cache.bytes = None;
                metrics.diagnostics.push(error.to_string());
                continue;
            }
        };
        if saving && cache.save == SaveOutcome::Unknown {
            cache.save = if entry.is_some() {
                SaveOutcome::Saved
            } else {
                SaveOutcome::Failed
            };
        }
        cache.bytes = entry.and_then(|entry| entry["size_in_bytes"].as_u64());
    }
    Ok(())
}

fn inventory_entry<'a>(
    listing: &'a Value,
    key: &str,
    saving: bool,
) -> Result<Option<&'a Value>, CiError> {
    let unknown = || ci(format!("Cache inventory identity unknown for {key}"));
    let entries = listing["actions_caches"].as_array().ok_or_else(unknown)?;
    if listing["total_count"].as_u64() != u64::try_from(entries.len()).ok() {
        return Err(unknown());
    }
    let mut refs = vec![env("GITHUB_REF")];
    if !saving {
        refs.extend(
            [env("GITHUB_BASE_REF"), env("VELNOR_REPORT_DEFAULT_BRANCH")]
                .into_iter()
                .filter(|branch| !branch.is_empty())
                .map(|branch| format!("refs/heads/{branch}")),
        );
    }
    let mut eligible = entries.iter().filter(|entry| {
        entry["key"].as_str() == Some(key)
            && entry["ref"]
                .as_str()
                .is_some_and(|reference| refs.iter().any(|allowed| allowed == reference))
    });
    let entry = eligible.next();
    if eligible.next().is_some() {
        return Err(unknown());
    }
    Ok(entry)
}

fn check_directory(directory: &Path) -> Result<String, CiError> {
    let mut files = Vec::new();
    find_metrics(directory, &mut files)?;
    let mut lanes = files
        .iter()
        .map(|file| {
            fs::read_to_string(file)
                .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))
                .and_then(|text| json::<LaneMetrics>(&text))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let needs: serde_json::Map<String, Value> =
        json(&std::env::var("VELNOR_REPORT_NEEDS").unwrap_or_else(|_| "{}".to_owned()))?;
    for (lane, need) in needs {
        if lane == "rust-trim" {
            if !matches!(need["result"].as_str(), Some("success" | "skipped")) {
                return Err(CiError::new("cache upload verification failed"));
            }
            continue;
        }
        if need["result"] != "skipped" && !lanes.iter().any(|metrics| metrics.lane == lane) {
            return Err(CiError::new(format!("missing metrics for lane {lane}")));
        }
    }
    for (file, lane) in files.iter().zip(&mut lanes) {
        if let Err(error) = api_metrics(lane) {
            lane.diagnostics
                .push(format!("GitHub telemetry unavailable: {error}"));
        }
        lane.violations = waste(lane);
        fs::write(file, serde_json::to_string_pretty(lane).map_err(ci)?).map_err(ci)?;
        super::append_to(
            "GITHUB_STEP_SUMMARY",
            &format!("## {}\n\n{}", lane.lane, lane.summary()),
        )?;
    }
    check(&lanes)
}
fn find_metrics(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), CiError> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?
    {
        let entry = entry
            .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?;
        if entry
            .file_type()
            .map_err(|error| CiError::new(format!("cannot write invocation records: {error}")))?
            .is_dir()
        {
            find_metrics(&entry.path(), files)?;
        } else if entry.file_name() == "ci-metrics.json" {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn activity_metrics(metrics: &mut LaneMetrics, log: &[u8]) {
    for invocation in &metrics.invocations {
        if !invocation.capture_complete {
            metrics
                .diagnostics
                .push(format!("{}: output capture incomplete", invocation.root));
        }
        let mut tap = crate::cargo_artifacts::ArtifactTap::default();
        tap.feed(
            log.get(invocation.log_start..invocation.log_end)
                .unwrap_or_default(),
        );
        let batch = tap.finish();
        metrics.downloaded += invocation.downloads.len() as u64;
        metrics
            .slowest_tests
            .extend(if invocation.progress.is_some() {
                invocation.tests.clone()
            } else {
                batch.tests
            });
        metrics
            .diagnostics
            .extend(crate::observed_units::diagnostics(invocation));
    }
    metrics
        .slowest_tests
        .sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    metrics.slowest_tests.truncate(10);
}
