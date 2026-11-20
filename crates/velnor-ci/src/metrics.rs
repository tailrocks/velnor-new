// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::logscan::LogScan;

pub const METRICS_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub schema_version: u32,
    pub repo: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub workflow: String,
    pub event: String,
    pub head_sha: String,
    pub wall_seconds: Option<u64>,
    pub runner_seconds: Option<u64>,
    pub jobs: Vec<JobMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobMetrics {
    pub name: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub conclusion: String,
    pub started: bool,
    pub queue_seconds: Option<u64>,
    pub duration_seconds: Option<u64>,
    #[serde(default)]
    pub setup_seconds: Option<u64>,
    pub steps: Vec<StepMetrics>,
    pub log: LogScan,
    /// The job's log had a different number of top-level step groups than the
    /// job has steps, so no step got its own log scan.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub step_logs_unmatched: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepMetrics {
    pub name: String,
    pub seconds: Option<u64>,
    #[serde(default)]
    pub log: LogScan,
}

impl JobMetrics {
    #[must_use]
    pub fn ran(&self) -> bool {
        self.started && self.conclusion != "skipped"
    }
}

impl RunMetrics {
    #[must_use]
    pub fn jobs_run(&self) -> usize {
        self.jobs.iter().filter(|job| job.ran()).count()
    }

    #[must_use]
    pub fn jobs_skipped(&self) -> usize {
        self.jobs.len() - self.jobs_run()
    }

    #[must_use]
    pub fn crates_compiled(&self) -> u64 {
        self.jobs.iter().map(|job| job.log.crates_compiled()).sum()
    }

    #[must_use]
    pub fn crates_downloaded(&self) -> u64 {
        self.jobs.iter().map(|job| job.log.downloaded).sum()
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let runner = self.runner_seconds.map_or_else(
            || "unmeasured".to_owned(),
            |seconds| format!("{seconds}s ({:.1} runner-minutes)", minutes(seconds)),
        );
        let mut lines = vec![
            format!(
                "run {} of {} (attempt {}, {}, {})",
                self.run_id, self.repo, self.run_attempt, self.event, self.head_sha
            ),
            format!(
                "wall {}, runner {runner}",
                self.wall_seconds
                    .map_or_else(|| "unmeasured".to_owned(), |seconds| format!("{seconds}s"))
            ),
            format!(
                "jobs {} ({} run, {} skipped), crates compiled {}, crates downloaded {}",
                self.jobs.len(),
                self.jobs_run(),
                self.jobs_skipped(),
                self.crates_compiled(),
                self.crates_downloaded()
            ),
        ];
        lines.extend(self.jobs.iter().map(|job| {
            let duration = job
                .duration_seconds
                .map_or_else(|| "unmeasured".to_owned(), |seconds| format!("{seconds}s"));
            let setup = job
                .setup_seconds
                .map_or_else(String::new, |seconds| format!(", setup {seconds}s"));
            format!(
                "  {} [{}] {duration}{setup}, crates compiled {}, downloaded {}",
                job.name,
                job.conclusion,
                job.log.crates_compiled(),
                job.log.downloaded
            )
        }));
        lines.join("\n") + "\n"
    }
}

fn minutes(seconds: u64) -> f64 {
    std::time::Duration::from_secs(seconds).as_secs_f64() / 60.0
}
