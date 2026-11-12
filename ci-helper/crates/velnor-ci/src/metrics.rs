// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::logscan::LogScan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub repo: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub event: String,
    pub head_sha: String,
    pub wall_seconds: u64,
    pub runner_seconds: u64,
    pub jobs: Vec<JobMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobMetrics {
    pub name: String,
    pub conclusion: String,
    pub queue_seconds: u64,
    pub duration_seconds: u64,
    pub steps: Vec<StepMetrics>,
    pub log: LogScan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepMetrics {
    pub name: String,
    pub seconds: u64,
}

impl JobMetrics {
    #[must_use]
    pub fn skipped(&self) -> bool {
        self.conclusion == "skipped"
    }
}

impl RunMetrics {
    #[must_use]
    pub fn jobs_run(&self) -> usize {
        self.jobs.iter().filter(|job| !job.skipped()).count()
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
        let mut lines = vec![
            format!(
                "run {} of {} (attempt {}, {}, {})",
                self.run_id, self.repo, self.run_attempt, self.event, self.head_sha
            ),
            format!(
                "wall {}s, runner {}s ({:.1} runner-minutes)",
                self.wall_seconds,
                self.runner_seconds,
                minutes(self.runner_seconds)
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
            format!(
                "  {} [{}] {}s, crates compiled {}, downloaded {}",
                job.name,
                job.conclusion,
                job.duration_seconds,
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
