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

#[test]
fn recorded_run_becomes_metrics() {
    let metrics = recorded_metrics();
    assert_eq!(metrics.repo, "tailrocks/velnor-new");
    assert_eq!(metrics.run_attempt, 1);
    assert_eq!(metrics.event, "push");
    assert_eq!(metrics.head_sha, "41f0a2dd1fee1a11c35a172ef1298fca69fc62e8");
    assert_eq!(metrics.wall_seconds, Some(963));
    assert_eq!(metrics.runner_seconds, Some(12 + 854 + 70));
    assert_eq!(metrics.jobs.len(), 3);
    let cli = &metrics.jobs[1];
    assert_eq!(cli.name, "Rust / velnor-actions-cli");
    assert_eq!(cli.conclusion, "success");
    assert_eq!(cli.queue_seconds, Some(2));
    assert_eq!(cli.duration_seconds, Some(854));
    assert_eq!(cli.steps.len(), 6);
    assert_eq!(cli.steps[1].name, "Checkout");
    assert_eq!(cli.steps[1].seconds, Some(5));
    assert_eq!(cli.log.compiling, 24);
    assert_eq!(metrics.crates_compiled(), 28);
}

#[test]
fn the_file_collect_writes_is_the_file_compare_reads() {
    let metrics = recorded_metrics();
    let path = std::env::temp_dir().join(format!("velnor-ci-metrics-{}.json", std::process::id()));
    velnor_ci::io::write_metrics(&path, &metrics).unwrap_or_else(|e| unreachable!("{e}"));
    let back = velnor_ci::io::read_metrics(&path).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(back, metrics);
    let missing = velnor_ci::io::read_metrics(&path).map(|_| ());
    assert!(missing.is_err());
}

#[test]
fn a_run_that_did_not_succeed_is_refused() {
    for conclusion in ["failure", "cancelled", "timed_out"] {
        let mut fake = FakeGitHub::recorded();
        let failed = RUN.replace(
            "\"conclusion\": \"success\"",
            &format!("\"conclusion\": \"{conclusion}\""),
        );
        assert_ne!(failed, RUN);
        fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), &failed);
        let message = error_text(collect(&fake, &repo(), RUN_ID, true));
        assert!(message.contains(conclusion), "{message}");
    }
}

#[test]
fn a_job_carried_over_from_an_earlier_attempt_is_not_a_job_run() {
    let mut fake = FakeGitHub::recorded();
    let rerun = RUN
        .replace("\"run_attempt\": 1", "\"run_attempt\": 2")
        .replace(
            "\"run_started_at\": \"2026-11-12T08:36:32Z\"",
            "\"run_started_at\": \"2026-11-12T08:37:00Z\"",
        );
    assert_ne!(rerun, RUN);
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), &rerun);
    fake.respond(jobs_path(&repo(), RUN_ID, 2, 1), JOBS);

    let metrics = collect(&fake, &repo(), RUN_ID, true).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs.len(), 3);
    assert_eq!(metrics.jobs_run(), 2);
    assert_eq!(metrics.runner_seconds, Some(854 + 70));
}

#[test]
fn a_run_that_has_not_completed_is_refused() {
    let mut fake = FakeGitHub::recorded();
    let running = RUN.replace("\"status\": \"completed\"", "\"status\": \"in_progress\"");
    assert_ne!(running, RUN);
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), &running);
    let message = error_text(collect(&fake, &repo(), RUN_ID, true));
    assert!(message.contains("in_progress"), "{message}");
}

#[test]
fn wall_time_starts_at_run_started_at_not_created_at() {
    let mut fake = FakeGitHub::recorded();
    let early = RUN.replace(
        "\"created_at\": \"2026-11-12T08:36:32Z\"",
        "\"created_at\": \"2026-11-12T08:00:00Z\"",
    );
    assert_ne!(early, RUN);
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), &early);
    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.wall_seconds, Some(963));
}

#[test]
fn rerun_is_refused_unless_allowed() {
    let mut fake = FakeGitHub::recorded();
    let rerun = RUN.replace("\"run_attempt\": 1", "\"run_attempt\": 2");
    assert_ne!(rerun, RUN);
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), &rerun);
    fake.respond(jobs_path(&repo(), RUN_ID, 2, 1), JOBS);

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
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &skipped);
    fake.respond(job_log_path(&repo(), CLI_ID), CLI_LOG);
    fake.respond(job_log_path(&repo(), CORE_ID), CORE_LOG);

    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs_skipped(), 1);
    assert_eq!(metrics.jobs_run(), 2);
    assert_eq!(metrics.runner_seconds, Some(854 + 70));
}

#[test]
fn a_job_cancelled_before_it_started_is_not_a_job_run() {
    let cancelled = JOBS
        .replacen(
            "\"conclusion\": \"success\"",
            "\"conclusion\": \"cancelled\"",
            1,
        )
        .replacen(
            "\"started_at\": \"2026-11-12T08:36:36Z\"",
            "\"started_at\": null",
            1,
        );
    assert_ne!(cancelled, JOBS);
    let mut fake = FakeGitHub::default();
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), RUN);
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &cancelled);
    fake.respond(job_log_path(&repo(), CLI_ID), CLI_LOG);
    fake.respond(job_log_path(&repo(), CORE_ID), CORE_LOG);

    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs.len(), 3);
    assert_eq!(metrics.jobs_run(), 2);
    assert_eq!(metrics.jobs_skipped(), 1);
    assert_eq!(metrics.jobs[0].queue_seconds, None);
    assert_eq!(metrics.jobs[0].duration_seconds, None);
    assert_eq!(metrics.runner_seconds, Some(854 + 70));
}

#[test]
fn jobs_on_later_pages_are_collected() {
    let first = JOBS.replace("\"total_count\": 3", "\"total_count\": 4");
    let second = r#"{"total_count": 4, "jobs": [{"id": 7, "name": "Late", "status": "completed",
        "conclusion": "success", "created_at": "2026-11-12T08:50:00Z",
        "started_at": "2026-11-12T08:50:10Z", "completed_at": "2026-11-12T08:50:40Z", "steps": []}]}"#;
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &first);
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 2), second);
    fake.respond(job_log_path(&repo(), 7), "");

    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs.len(), 4);
    assert_eq!(metrics.jobs[3].name, "Late");
    assert_eq!(metrics.jobs[3].queue_seconds, Some(10));
    assert_eq!(metrics.jobs[3].duration_seconds, Some(30));
}

#[test]
fn a_missing_job_log_fails_the_collection_and_names_the_job() {
    let mut fake = FakeGitHub::default();
    fake.respond(velnor_ci::github::run_path(&repo(), RUN_ID), RUN);
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), JOBS);
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

#[test]
fn unmeasured_run_and_job_timestamps_stay_distinct_from_measured_zero() {
    let unmeasured = collected_with_times(
        "2026-11-12T08:36:32Z",
        "not-a-time",
        "2026-11-12T08:36:32Z",
        "not-a-time",
        "not-a-time",
    );
    let summary = unmeasured.summary();
    assert_eq!(unmeasured.wall_seconds, None);
    assert_eq!(unmeasured.runner_seconds, None);
    assert!(
        unmeasured
            .jobs
            .iter()
            .filter(|job| job.ran())
            .all(|job| { job.queue_seconds.is_none() && job.duration_seconds.is_none() })
    );
    assert!(
        summary.contains("wall unmeasured, runner unmeasured"),
        "{summary}"
    );
    assert!(summary.contains("[success] unmeasured"), "{summary}");

    let measured_zero = collected_with_times(
        "2026-11-12T08:36:32Z",
        "2026-11-12T08:36:32Z",
        "2026-11-12T08:36:32Z",
        "2026-11-12T08:36:32Z",
        "2026-11-12T08:36:32Z",
    );
    assert_eq!(measured_zero.wall_seconds, Some(0));
    assert_eq!(measured_zero.runner_seconds, Some(0));
    assert!(
        measured_zero
            .jobs
            .iter()
            .filter(|job| job.ran())
            .all(|job| job.queue_seconds == Some(0) && job.duration_seconds == Some(0))
    );
    assert!(
        measured_zero
            .summary()
            .contains("wall 0s, runner 0s (0.0 runner-minutes)")
    );
    let comparison = compare(&unmeasured, &measured_zero).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        comparison.contains("| Wall seconds | - | 0 | - |"),
        "{comparison}"
    );
    assert!(
        comparison.contains("| Runner seconds | - | 0 | - |"),
        "{comparison}"
    );
    assert!(comparison.contains("unmeasured | 0 | -"), "{comparison}");
}

#[test]
fn setup_ends_when_the_restores_are_done_or_for_the_plan_job_when_the_plan_is() {
    let lane = job_with(&[
        ("Check out", "success", "2026-11-15T10:00:03Z"),
        ("velnor-ci prepare", "success", "2026-11-15T10:00:20Z"),
        ("velnor-ci restored", "success", "2026-11-15T10:00:24Z"),
        ("Format", "success", "2026-11-15T10:03:00Z"),
    ]);
    assert_eq!(velnor_ci::collect::setup_seconds(&lane), Some(24));
    let plan = job_with(&[
        ("Check out", "success", "2026-11-15T10:00:03Z"),
        ("velnor-ci plan", "success", "2026-11-15T10:00:09Z"),
    ]);
    assert_eq!(velnor_ci::collect::setup_seconds(&plan), Some(9));
    let skipped = job_with(&[("velnor-ci restored", "skipped", "2026-11-15T10:00:24Z")]);
    assert_eq!(velnor_ci::collect::setup_seconds(&skipped), None);
    let nothing_selected = job_with(&[
        ("velnor-ci plan", "success", "2026-11-15T10:00:09Z"),
        ("velnor-ci restored", "skipped", "2026-11-15T10:00:09Z"),
    ]);
    assert_eq!(velnor_ci::collect::setup_seconds(&nothing_selected), None);
    assert_eq!(velnor_ci::collect::setup_seconds(&job_with(&[])), None);
}
