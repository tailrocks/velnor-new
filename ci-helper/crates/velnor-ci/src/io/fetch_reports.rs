// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use super::artifact_api::ArtifactApi;
use super::artifact_unzip::{Budget, extract};
use super::read_limited;
use crate::cli::FetchReportsArgs;
use crate::error::CiError;
use crate::github::Repo;
use crate::reports::{expected_artifacts, select};

const PLAN_LIMIT: u64 = 8 << 20;

struct Fetched {
    failures: Vec<String>,
    bytes: u64,
}

pub(super) fn run_fetch_reports(args: &FetchReportsArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let plan_path = args.run_dir.join("plan.json");
    let plan = read_plan(&plan_path)?;
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
    let budget = Budget::new(args.max_extract_bytes);
    let fetched = download_all(
        &api,
        &repo,
        &reports,
        &selection.wanted,
        (connections, &budget),
    );
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
    (connections, budget): (usize, &Budget),
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
                        match fetch_one(api, repo, (reports, budget), name, *id) {
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
    (reports, budget): (&Path, &Budget),
    name: &str,
    id: u64,
) -> Result<u64, CiError> {
    let bytes = api.download(repo, id)?;
    extract(&bytes, name, reports, budget)?;
    Ok(bytes.len() as u64)
}

/// Reads plan.json without following a symbolic link or blocking on a FIFO, and only when it is a
/// regular file within `PLAN_LIMIT`.
fn read_plan(path: &Path) -> Result<String, CiError> {
    let unreadable = |reason: &dyn std::fmt::Display| {
        CiError::new(format!("cannot read {}: {reason}", path.display()))
    };
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| unreadable(&e))?;
    if !file.metadata().map_err(|e| unreadable(&e))?.is_file() {
        return Err(unreadable(&"not a regular file"));
    }
    let bytes = read_limited(file, PLAN_LIMIT)
        .map_err(|_| unreadable(&format!("larger than {PLAN_LIMIT} bytes")))?;
    String::from_utf8(bytes).map_err(|_| unreadable(&"not UTF-8"))
}
