// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::collect::collect;
use velnor_ci::github::{Repo, job_log_path, jobs_path};

use crate::support::{
    CLI_ID, CLI_LOG, CORE_ID, CORE_LOG, FakeGitHub, JOBS, RUN, RUN_ID, error_text,
    recorded_metrics, repo,
};

#[test]
fn recorded_run_becomes_metrics() {
    let metrics = recorded_metrics();
    assert_eq!(metrics.repo, "tailrocks/velnor-new");
    assert_eq!(metrics.run_attempt, 1);
    assert_eq!(metrics.event, "push");
    assert_eq!(metrics.head_sha, "41f0a2dd1fee1a11c35a172ef1298fca69fc62e8");
    assert_eq!(metrics.wall_seconds, 963);
    assert_eq!(metrics.runner_seconds, 12 + 854 + 70);
    assert_eq!(metrics.jobs.len(), 3);
    let cli = &metrics.jobs[1];
    assert_eq!(cli.name, "Rust / velnor-actions-cli");
    assert_eq!(cli.conclusion, "success");
    assert_eq!(cli.queue_seconds, 2);
    assert_eq!(cli.duration_seconds, 854);
    assert_eq!(cli.steps.len(), 6);
    assert_eq!(cli.steps[1].name, "Checkout");
    assert_eq!(cli.steps[1].seconds, 5);
    assert_eq!(cli.log.compiling, 24);
    assert_eq!(metrics.crates_compiled(), 28);
}

#[test]
fn metrics_survive_a_json_round_trip() {
    let metrics = recorded_metrics();
    let json = serde_json::to_string(&metrics).unwrap_or_else(|e| unreachable!("{e}"));
    let back: velnor_ci::metrics::RunMetrics =
        serde_json::from_str(&json).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(back, metrics);
}

#[test]
fn rerun_is_refused_unless_allowed() {
    let mut fake = FakeGitHub::recorded();
    let rerun = RUN.replace("\"run_attempt\": 1", "\"run_attempt\": 2");
    assert_ne!(rerun, RUN);
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), &rerun);

    let refused = collect(&fake, &repo(), RUN_ID, false);
    let message = error_text(refused);
    assert!(
        message.contains("attempt 2") && message.contains("--allow-rerun"),
        "{message}"
    );

    let allowed = collect(&fake, &repo(), RUN_ID, true).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(allowed.run_attempt, 2);
}

#[test]
fn skipped_jobs_cost_nothing_and_have_no_log_fetched() {
    let skipped = JOBS.replacen(
        "\"conclusion\": \"success\"",
        "\"conclusion\": \"skipped\"",
        1,
    );
    assert_ne!(skipped, JOBS);
    let mut fake = FakeGitHub::default();
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), RUN);
    fake.respond(jobs_path(&repo(), RUN_ID, 1), &skipped);
    fake.respond(job_log_path(&repo(), CLI_ID), CLI_LOG);
    fake.respond(job_log_path(&repo(), CORE_ID), CORE_LOG);

    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs_skipped(), 1);
    assert_eq!(metrics.jobs_run(), 2);
    assert_eq!(metrics.runner_seconds, 854 + 70);
}

#[test]
fn jobs_on_later_pages_are_collected() {
    let first = JOBS.replace("\"total_count\": 3", "\"total_count\": 4");
    let second = r#"{"total_count": 4, "jobs": [{"id": 7, "name": "Late", "status": "completed",
        "conclusion": "success", "created_at": "2026-11-12T08:50:00Z",
        "started_at": "2026-11-12T08:50:10Z", "completed_at": "2026-11-12T08:50:40Z", "steps": []}]}"#;
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1), &first);
    fake.respond(jobs_path(&repo(), RUN_ID, 2), second);
    fake.respond(job_log_path(&repo(), 7), "");

    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs.len(), 4);
    assert_eq!(metrics.jobs[3].name, "Late");
    assert_eq!(metrics.jobs[3].queue_seconds, 10);
    assert_eq!(metrics.jobs[3].duration_seconds, 30);
}

#[test]
fn a_missing_job_log_fails_the_collection_and_names_the_job() {
    let mut fake = FakeGitHub::default();
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), RUN);
    fake.respond(jobs_path(&repo(), RUN_ID, 1), JOBS);
    let message = error_text(collect(&fake, &repo(), RUN_ID, false));
    assert!(message.contains("Cargo Machete"), "{message}");
}

#[test]
fn repository_must_be_owner_slash_name() {
    assert!(Repo::parse("tailrocks/velnor-new").is_ok());
    for bad in ["velnor-new", "/x", "x/", "a/b/c", ""] {
        assert!(Repo::parse(bad).is_err(), "{bad}");
    }
}
