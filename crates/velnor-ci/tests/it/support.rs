// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use velnor_ci::collect::collect;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, Repo, job_log_path, jobs_path, run_path};
use velnor_ci::metrics::RunMetrics;

pub(crate) const RUN: &str = include_str!("data/run.json");
pub(crate) const JOBS: &str = include_str!("data/jobs.json");
pub(crate) const CLI_LOG: &str = include_str!("data/cli-job.log");
pub(crate) const CORE_LOG: &str = include_str!("data/core-job.log");
pub(crate) const MACHETE_LOG: &str = "2026-11-12T08:36:40.0000000Z Running cargo machete\n";

pub(crate) const RUN_ID: u64 = 43_184_351_757;
pub(crate) const MACHETE_ID: u64 = 131_067_729_993;
pub(crate) const CLI_ID: u64 = 131_068_183_290;
pub(crate) const CORE_ID: u64 = 131_068_183_788;

#[derive(Debug, Default)]
pub(crate) struct FakeGitHub {
    responses: BTreeMap<String, String>,
    absent: BTreeSet<String>,
}

impl FakeGitHub {
    pub(crate) fn recorded() -> Self {
        let repo = repo();
        let mut fake = Self::default();
        fake.respond(run_path(&repo, RUN_ID), RUN);
        fake.respond(jobs_path(&repo, RUN_ID, 1, 1), JOBS);
        fake.respond(job_log_path(&repo, MACHETE_ID), MACHETE_LOG);
        fake.respond(job_log_path(&repo, CLI_ID), CLI_LOG);
        fake.respond(job_log_path(&repo, CORE_ID), CORE_LOG);
        fake
    }

    pub(crate) fn get_text(&self, path: &str) -> String {
        self.responses.get(path).cloned().unwrap_or_default()
    }

    pub(crate) fn missing(&mut self, path: String) {
        self.absent.insert(path);
    }

    pub(crate) fn respond(&mut self, path: String, body: &str) {
        self.responses.insert(path, body.to_owned());
    }
}

impl GitHub for FakeGitHub {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if self.absent.contains(path) {
            return Err(CiError::missing(format!("GET {path}: http status: 404")));
        }
        self.responses
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::new(format!("no recorded response for {path}")))
    }
}

pub(crate) fn repo() -> Repo {
    Repo::parse("tailrocks/velnor-new").unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn error_text(result: Result<RunMetrics, CiError>) -> String {
    match result {
        Ok(_) => "unexpected success".to_owned(),
        Err(error) => error.to_string(),
    }
}

pub(crate) fn recorded_metrics() -> RunMetrics {
    collect(&FakeGitHub::recorded(), &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) const DEAD_PID: i32 = 2_000_000_000;

pub(crate) fn assert_dead(pid: i32) {
    assert_eq!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH));
}

pub(crate) static CREATED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn fresh_dir(prefix: &str, name: &str) -> PathBuf {
    fresh_dir_in(&std::env::temp_dir(), &CREATED, prefix, name)
}

pub(crate) fn fresh_dir_in(root: &Path, counter: &AtomicU64, prefix: &str, name: &str) -> PathBuf {
    sweep_dead(root, prefix);
    loop {
        let candidate = root.join(format!(
            "{prefix}-{}-{}-{name}",
            std::process::id(),
            counter.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => unreachable!("{error}"),
        }
    }
}

fn sweep_dead(root: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| dead_pid_candidate(name, prefix))
        else {
            continue;
        };
        if !is_running(pid) && entry.path().is_dir() {
            let _ignored = fs::remove_dir_all(entry.path());
        }
    }
}

fn dead_pid_candidate(name: &str, prefix: &str) -> Option<i32> {
    let rest = name.strip_prefix(prefix)?.strip_prefix('-')?;
    let (pid, tail) = rest.split_once('-')?;
    if tail.is_empty() || pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

fn is_running(pid: i32) -> bool {
    kill(Pid::from_raw(pid), None) != Err(Errno::ESRCH)
}
