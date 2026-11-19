// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use super::artifact_api::ArtifactApi;
use super::artifact_unzip::extract;
use crate::cli::FetchReportsArgs;
use crate::error::CiError;
use crate::github::Repo;
use crate::reports::{expected_artifacts, select};

const ATTEMPTS: u32 = 3;

struct Fetched {
    failures: Vec<String>,
    bytes: u64,
}

pub(super) fn run_fetch_reports(args: &FetchReportsArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let plan_path = args.run_dir.join("plan.json");
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", plan_path.display())))?;
    let expected = expected_artifacts(&plan)?;
    if expected.is_empty() {
        return Ok("plan.json expects no report artifacts\n".to_owned());
    }
    let api = ArtifactApi::new(super::api_url(), super::token()?);
    let started = Instant::now();
    let selection = select(&expected, &api.list(&repo, args.run_id)?);
    let reports = args.run_dir.join("reports");
    fs::create_dir_all(&reports)
        .map_err(|e| CiError::new(format!("cannot create {}: {e}", reports.display())))?;
    let connections = usize::from(args.connections);
    let fetched = download_all(&api, &repo, &reports, &selection.wanted, connections);
    let mut problems = selection.problems;
    problems.extend(fetched.failures);
    if !problems.is_empty() {
        problems.sort();
        return Err(CiError::new(format!(
            "{} of {} expected report artifacts are unavailable: {}",
            problems.len(),
            expected.len(),
            problems.join("; ")
        )));
    }
    Ok(format!(
        "fetched {} report artifacts ({} KiB) over {} connections in {:.1}s\n",
        expected.len(),
        fetched.bytes / 1024,
        connections.min(selection.wanted.len()),
        started.elapsed().as_secs_f32()
    ))
}

fn download_all(
    api: &ArtifactApi,
    repo: &Repo,
    reports: &Path,
    wanted: &[(String, u64)],
    connections: usize,
) -> Fetched {
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        let workers: Vec<_> = (0..connections.min(wanted.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Fetched {
                        failures: Vec::new(),
                        bytes: 0,
                    };
                    while let Some((name, id)) = wanted.get(next.fetch_add(1, Ordering::Relaxed)) {
                        match fetch_one(api, repo, reports, name, *id) {
                            Ok(bytes) => done.bytes += bytes,
                            Err(error) => done.failures.push(format!("{name}: {error}")),
                        }
                    }
                    done
                })
            })
            .collect();
        let mut all = Fetched {
            failures: Vec::new(),
            bytes: 0,
        };
        for worker in workers {
            match worker.join() {
                Ok(done) => {
                    all.failures.extend(done.failures);
                    all.bytes += done.bytes;
                }
                Err(_) => all.failures.push("a download worker panicked".to_owned()),
            }
        }
        all
    })
}

fn fetch_one(
    api: &ArtifactApi,
    repo: &Repo,
    reports: &Path,
    name: &str,
    id: u64,
) -> Result<u64, CiError> {
    let mut attempt = 1;
    let bytes = loop {
        match api.download(repo, id) {
            Ok(bytes) => break bytes,
            Err(error) if error.is_missing() || attempt == ATTEMPTS => {
                return Err(error.context(&format!("download failed after {attempt} attempts")));
            }
            Err(_) => attempt += 1,
        }
    };
    extract(&bytes, name, reports)?;
    Ok(bytes.len() as u64)
}
