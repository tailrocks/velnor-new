use super::*;

#[test]
fn an_affected_run_is_not_judged_by_the_limits_of_full_runs_and_the_reverse() {
    let histories = vec![
        history_of(
            Population::MainPush,
            Selection::Full,
            vec![sample(3, 800, 900, None), sample(2, 700, 800, None)],
        ),
        history_of(
            Population::MainPush,
            Selection::Affected,
            vec![sample(5, 90, 45, None), sample(4, 80, 45, None)],
        ),
    ];
    let evaluation = evaluate(&two_modes(), &histories);
    assert!(evaluation.passed(), "{evaluation:?}");

    let slow_affected = vec![history_of(
        Population::MainPush,
        Selection::Affected,
        vec![sample(6, 150, 45, None)],
    )];
    let evaluation = evaluate(&two_modes(), &slow_affected);
    assert_eq!(
        breached(&evaluation),
        [(Some(Selection::Affected), 150, 100)],
        "800 seconds is fine for a full run and not for an affected one"
    );
}

#[test]
fn a_run_with_an_unknown_selection_is_judged_by_the_plain_keys_only() {
    let mut limits = two_modes();
    limits.main_wall_seconds = 200;
    let unknown = vec![history(Population::MainPush, vec![sample(9, 300, 0, None)])];
    let evaluation = evaluate(&limits, &unknown);
    assert_eq!(breached(&evaluation), [(None, 300, 200)]);
}

#[test]
fn a_full_run_after_only_affected_history_passes_within_its_own_limit() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1, 2, 3]);
    for id in [1, 2, 3] {
        finished(&mut fake, id, 60, Some("Speed budget (affected)"));
    }
    let running = executing(&mut fake, FULL_RUN, 700, Some("Speed budget (full)"));
    assert_eq!(running.sample.selection, Some(Selection::Full));
    let (ids, evaluation) = judged(&fake, &running, &two_modes());
    assert_eq!(ids, [FULL_RUN], "no affected run is a sample of a full run");
    assert!(evaluation.passed(), "{evaluation:?}");
}

#[test]
fn runs_that_do_not_record_their_selection_are_one_group_of_their_own() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1, 2, 3]);
    finished(&mut fake, 1, 800, None);
    finished(&mut fake, 2, 60, Some("Speed budget (affected)"));
    finished(&mut fake, 3, 790, None);
    let running = executing(&mut fake, FULL_RUN, 810, None);
    assert_eq!(running.sample.selection, None);
    let (ids, _) = judged(&fake, &running, &budget(0, 900, 0, 0));
    assert_eq!(ids, [FULL_RUN, 1, 3]);
}

#[test]
fn a_skipped_budget_job_records_nothing() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1]);
    fake.respond(
        run_path(&repo(), 1),
        &json!({
            "name": "Rust CI", "status": "completed", "conclusion": "success",
            "run_attempt": 1, "event": "push", "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(60)
        })
        .to_string(),
    );
    let mut skipped = budget_job(Some("Speed budget (full)"), 1, None);
    skipped["conclusion"] = json!("skipped");
    fake.respond(
        jobs_path(&repo(), 1, 1, 1),
        &json!({ "total_count": 2, "jobs": [rust_job(50), skipped] }).to_string(),
    );
    let measured = velnor_ci::budget_history::sample(&fake, &repo(), 1)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(measured.selection, None);
}

#[test]
fn an_unknown_key_in_a_mode_table_or_an_unknown_mode_is_refused() {
    for (config, key) in [
        (
            "[budget.full]\nmain_wall_second = 900\n",
            "budget.full.main_wall_second",
        ),
        (
            "[budget.everything]\nmain_wall_seconds = 900\n",
            "budget.everything",
        ),
    ] {
        let error = velnor_rust::budget(config.as_bytes()).expect_err("the key was accepted");
        assert!(error.to_string().contains(key), "{error}");
    }
}
