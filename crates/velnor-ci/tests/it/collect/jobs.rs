use super::*;

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
fn jobs_missing_from_the_advertised_count_are_refused() {
    let advertised = r#"{"total_count": 1, "jobs": []}"#;
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), advertised);
    let message = error_text(collect(&fake, &repo(), RUN_ID, false));
    assert!(message.contains("incomplete"), "{message}");
}

#[test]
fn a_count_that_changes_between_pages_is_refused() {
    let first = JOBS.replace("\"total_count\": 3", "\"total_count\": 4");
    let second = r#"{"total_count": 3, "jobs": [{"id": 7, "name": "Late", "status": "completed",
        "conclusion": "success", "created_at": "2026-11-12T08:50:00Z",
        "started_at": "2026-11-12T08:50:10Z", "completed_at": "2026-11-12T08:50:40Z", "steps": []}]}"#;
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &first);
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 2), second);
    fake.respond(job_log_path(&repo(), 7), "");
    let message = error_text(collect(&fake, &repo(), RUN_ID, false));
    assert!(message.contains("changed during pagination"), "{message}");
}

#[test]
fn an_advertised_count_of_zero_is_complete() {
    let none = r#"{"total_count": 0, "jobs": []}"#;
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), none);
    let metrics = collect(&fake, &repo(), RUN_ID, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metrics.jobs.len(), 0);
    assert_eq!(metrics.runner_seconds, Some(0));
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
