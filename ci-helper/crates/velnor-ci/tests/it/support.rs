// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::collect::collect;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, Repo, job_log_path, jobs_path, run_path};
use velnor_ci::metrics::RunMetrics;

pub(crate) const RUN: &str = include_str!("data/run.json");
pub(crate) const JOBS: &str = include_str!("data/jobs.json");
pub(crate) const CLI_LOG: &str = include_str!("data/cli-job.log");
pub(crate) const CORE_LOG: &str = include_str!("data/core-job.log");
pub(crate) const GATE: &str = include_str!("data/gate.json");
pub(crate) const MACHETE_LOG: &str = "2026-11-12T08:36:40.0000000Z Running cargo machete\n";

pub(crate) const RUN_ID: u64 = 43_184_351_757;
pub(crate) const MACHETE_ID: u64 = 131_067_729_993;
pub(crate) const CLI_ID: u64 = 131_068_183_290;
pub(crate) const CORE_ID: u64 = 131_068_183_788;

#[derive(Debug, Default)]
pub(crate) struct FakeGitHub {
    responses: BTreeMap<String, String>,
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

    pub(crate) fn gate() -> Self {
        let recorded: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(GATE).unwrap_or_else(|e| unreachable!("{e}"));
        let mut fake = Self::default();
        for (path, body) in recorded {
            fake.respond(path, &body.to_string());
        }
        fake
    }

    pub(crate) fn respond(&mut self, path: String, body: &str) {
        self.responses.insert(path, body.to_owned());
    }
}

impl GitHub for FakeGitHub {
    fn get(&self, path: &str) -> Result<String, CiError> {
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
