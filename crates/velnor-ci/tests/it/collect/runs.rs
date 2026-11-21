use super::*;

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
fn an_empty_page_before_the_advertised_count_is_refused() {
    let first = JOBS.replace("\"total_count\": 3", "\"total_count\": 4");
    let second = r#"{"total_count": 4, "jobs": []}"#;
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &first);
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 2), second);
    let message = error_text(collect(&fake, &repo(), RUN_ID, false));
    assert!(message.contains("incomplete"), "{message}");
}

#[test]
fn more_jobs_than_the_advertised_count_are_refused() {
    let fewer = JOBS.replace("\"total_count\": 3", "\"total_count\": 2");
    let mut fake = FakeGitHub::recorded();
    fake.respond(jobs_path(&repo(), RUN_ID, 1, 1), &fewer);
    let message = error_text(collect(&fake, &repo(), RUN_ID, false));
    assert!(
        message.contains("refusing incomplete jobs data"),
        "{message}"
    );
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
