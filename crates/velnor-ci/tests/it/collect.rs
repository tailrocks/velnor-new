// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::collect::collect;
use velnor_ci::compare::compare;
use velnor_ci::github::{Repo, job_log_path, jobs_path, run_path};

use crate::support::{
    CLI_ID, CLI_LOG, CORE_ID, CORE_LOG, FakeGitHub, JOBS, RUN, RUN_ID, error_text,
    recorded_metrics, repo,
};

mod jobs;
mod runs;

fn job_with(steps: &[(&str, &str, &str)]) -> velnor_ci::github::ApiJob {
    let steps: Vec<String> = steps
        .iter()
        .map(|(name, conclusion, done)| {
            format!(
                "{{\"name\":\"{name}\",\"conclusion\":\"{conclusion}\",\"started_at\":\"2026-11-15T10:00:00Z\",\"completed_at\":\"{done}\"}}"
            )
        })
        .collect();
    serde_json::from_str(&format!(
        "{{\"id\":1,\"name\":\"rust\",\"conclusion\":\"success\",\"created_at\":\"2026-11-15T09:59:00Z\",\"started_at\":\"2026-11-15T10:00:00Z\",\"completed_at\":\"2026-11-15T10:05:00Z\",\"steps\":[{}]}}",
        steps.join(",")
    ))
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn collected_with_times(
    run_started: &str,
    run_updated: &str,
    job_created: &str,
    job_started: &str,
    job_completed: &str,
) -> velnor_ci::metrics::RunMetrics {
    let mut run: Value = serde_json::from_str(RUN).unwrap_or_else(|e| unreachable!("{e}"));
    run["run_started_at"] = json!(run_started);
    run["updated_at"] = json!(run_updated);
    let mut jobs: Value = serde_json::from_str(JOBS).unwrap_or_else(|e| unreachable!("{e}"));
    for job in jobs["jobs"].as_array_mut().into_iter().flatten() {
        if job["conclusion"] == "success" {
            job["created_at"] = json!(job_created);
            job["started_at"] = json!(job_started);
            job["completed_at"] = json!(job_completed);
        }
    }
    let mut fake = FakeGitHub::recorded();
    fake.respond(run_path(&repo(), RUN_ID), &run.to_string());
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &jobs.to_string());
    collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"))
}
