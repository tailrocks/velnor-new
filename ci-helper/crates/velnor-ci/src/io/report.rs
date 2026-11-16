// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cli::ReportArgs;
use crate::error::CiError;
use crate::github::{ApiJobs, GitHub, Repo, jobs_path};
use crate::logscan::{individual_test, scan};
use crate::report::{
    Hit, INVOCATIONS, Invocation, LaneMetrics, SCHEMA, check, configuration, waste,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
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

pub(super) fn start_invocation(argv: &[String]) -> Result<Invocation, CiError> {
    Ok(Invocation {
        argv: argv.to_vec(),
        root: super::current_dir()?.to_string_lossy().into_owned(),
        configuration: format!(
            "{} {:?}",
            configuration(argv),
            std::env::vars()
                .filter(|(key, _)| [
                    "RUSTUP_TOOLCHAIN",
                    "RUSTFLAGS",
                    "CARGO_ENCODED_RUSTFLAGS",
                    "CARGO_BUILD_TARGET"
                ]
                .contains(&key.as_str())
                    || key.starts_with("CARGO_PROFILE_"))
                .collect::<BTreeMap<_, _>>()
        ),
        started_at: u64::try_from(super::now_seconds()).unwrap_or(0),
        seconds: 0.0,
        log_start: 0,
        log_end: 0,
        compilations: Vec::new(),
        downloads: Vec::new(),
    })
}
pub(super) fn save_invocation(invocation: &Invocation) -> Result<(), CiError> {
    writeln!(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(temp(INVOCATIONS))
            .map_err(ci)?,
        "{}",
        serde_json::to_string(invocation).map_err(ci)?
    )
    .map_err(ci)
}

pub(super) fn run_report(args: &ReportArgs) -> Result<String, CiError> {
    if let Some(directory) = &args.check {
        return check_directory(directory);
    }
    let log = fs::read_to_string(temp("velnor-run.log")).unwrap_or_default();
    let invocations = fs::read_to_string(temp(INVOCATIONS))
        .unwrap_or_default()
        .lines()
        .map(json)
        .collect::<Result<Vec<Invocation>, _>>()?;
    let mut metrics = LaneMetrics {
        schema: SCHEMA,
        lane: env("GITHUB_JOB"),
        caches: json(&std::env::var("VELNOR_REPORT_CACHES").unwrap_or_else(|_| "[]".to_owned()))?,
        invocations,
        downloaded: scan(&log).downloaded,
        first_cargo_seconds: None,
        slowest_tests: log.lines().filter_map(individual_test).collect(),
        tests_run: env("VELNOR_TESTS_RUN").parse().ok(),
        diagnostics: Vec::new(),
        violations: Vec::new(),
    };
    metrics.invocations.retain(|invocation| {
        invocation.argv.first().is_some_and(|program| {
            Path::new(program)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("cargo"))
        })
    });
    metrics
        .slowest_tests
        .sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    metrics.slowest_tests.truncate(10);
    if let Err(error) = super::report_inputs::collect_inputs(&mut metrics, &log) {
        metrics
            .diagnostics
            .push(format!("Restored input provenance unavailable: {error}"));
    }
    let steps: Value =
        json(&std::env::var("VELNOR_REPORT_STEPS").unwrap_or_else(|_| "{}".to_owned()))?;
    for cache in &mut metrics.caches {
        cache.matched_key = steps[&cache.restore_step]["outputs"]["cache-matched-key"]
            .as_str()
            .filter(|key| !key.is_empty())
            .map(str::to_owned);
        cache.hit = match &cache.matched_key {
            Some(key) if key == &cache.requested_key => Hit::Exact,
            Some(_) => Hit::Fallback,
            None => Hit::Miss,
        };
        cache.usable &= cache.hit != Hit::Miss;
        cache.usable_same_lock = cache.usable
            && cache.matched_key.as_ref().is_some_and(|key| {
                cache
                    .lock_prefix
                    .as_ref()
                    .is_some_and(|prefix| key.starts_with(prefix))
            });
        cache.save_failed = env("GITHUB_REF")
            == format!("refs/heads/{}", env("VELNOR_REPORT_DEFAULT_BRANCH"))
            && matches!(
                steps[&cache.save_step]["outcome"].as_str(),
                Some("success" | "failure" | "cancelled")
            );
    }
    match api_metrics(&mut metrics, &steps) {
        Ok(()) => {}
        Err(error) => metrics
            .diagnostics
            .push(format!("GitHub telemetry unavailable: {error}")),
    }
    metrics.violations = waste(&metrics);
    let summary = metrics.summary();
    fs::write(
        temp("ci-metrics.json"),
        serde_json::to_string_pretty(&metrics).map_err(ci)?,
    )
    .map_err(ci)?;
    super::append_to("GITHUB_STEP_SUMMARY", &summary)?;
    Ok(summary)
}

fn api_metrics(metrics: &mut LaneMetrics, steps: &Value) -> Result<(), CiError> {
    let repo = Repo::parse(&env("GITHUB_REPOSITORY"))?;
    let github = super::http::HttpGitHub::new(
        super::api_url(),
        super::token()?,
        super::http::DEFAULT_TIMEOUT,
    );
    let run = env("GITHUB_RUN_ID").parse().map_err(ci)?;
    let attempt = env("GITHUB_RUN_ATTEMPT").parse().map_err(ci)?;
    for page in 1.. {
        let jobs: ApiJobs = json(&github.get(&jobs_path(&repo, run, attempt, page))?)?;
        if let Some(job) = jobs.jobs.iter().find(|job| {
            job.name
                == std::env::var("VELNOR_REPORT_JOB_NAME").unwrap_or_else(|_| metrics.lane.clone())
        }) {
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
    let default = env("VELNOR_REPORT_DEFAULT_BRANCH");
    for cache in &mut metrics.caches {
        let saving = env("GITHUB_REF") == format!("refs/heads/{default}")
            && matches!(
                steps[&cache.save_step]["outcome"].as_str(),
                Some("success" | "failure" | "cancelled")
            );
        let key = if saving {
            &cache.requested_key
        } else if let Some(key) = &cache.matched_key {
            key
        } else {
            continue;
        };
        let listing: Value = json(&github.get(&format!(
            "/repos/{repo}/actions/caches?key={}&per_page=100",
            crate::cache::encode(key)
        ))?)?;
        let entry = listing["actions_caches"].as_array().and_then(|entries| {
            entries.iter().find(|entry| {
                entry["key"].as_str() == Some(key)
                    && (!saving || entry["ref"].as_str() == Some(&env("GITHUB_REF")))
            })
        });
        cache.save_failed =
            saving && (entry.is_none() || steps[&cache.save_step]["outcome"] != "success");
        cache.bytes = entry.and_then(|entry| entry["size_in_bytes"].as_u64());
    }
    Ok(())
}

fn check_directory(directory: &Path) -> Result<String, CiError> {
    let mut files = Vec::new();
    find_metrics(directory, &mut files)?;
    let lanes = files
        .iter()
        .map(|file| {
            fs::read_to_string(file)
                .map_err(ci)
                .and_then(|text| json::<LaneMetrics>(&text))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let needs: Value =
        json(&std::env::var("VELNOR_REPORT_NEEDS").unwrap_or_else(|_| "{}".to_owned()))?;
    if let Some(needs) = needs.as_object() {
        for (lane, need) in needs {
            if need["result"] != "skipped" && !lanes.iter().any(|metrics| &metrics.lane == lane) {
                return Err(CiError::new(format!("missing metrics for lane {lane}")));
            }
        }
    }
    check(&lanes)
}
fn find_metrics(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), CiError> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(ci)? {
        let entry = entry.map_err(ci)?;
        if entry.file_type().map_err(ci)?.is_dir() {
            find_metrics(&entry.path(), files)?;
        } else if entry.file_name() == "ci-metrics.json" {
            files.push(entry.path());
        }
    }
    Ok(())
}
