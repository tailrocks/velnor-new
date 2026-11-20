use super::*;

#[test]
fn a_finished_job_without_a_completion_time_is_a_missing_measurement_not_zero_seconds() {
    let mut fake = FakeGitHub::default();
    run(&mut fake, 7);
    jobs(
        &mut fake,
        7,
        1,
        2,
        &[
            job("fast", Some("2026-11-01T10:01:10Z")),
            job("broken", None),
        ],
    );
    let error = sample(&fake, &repo(), 7).expect_err("a job without completed_at was accepted");
    assert!(error.to_string().contains("broken"), "{error}");
    assert!(
        error.to_string().contains("completed_at missing"),
        "{error}"
    );
}

#[test]
fn the_run_history_stops_paging_once_every_population_has_enough_runs() {
    let mut fake = FakeGitHub::default();
    let pages = |event: &str, first: u64| -> Vec<Value> {
        (0..100).map(|n| listed(first + n, event, "main")).collect()
    };
    listing(
        &mut fake,
        "pull_request",
        None,
        1,
        &pages("pull_request", 1000),
    );
    listing(&mut fake, "push", Some("main"), 1, &pages("push", 2000));
    listing(
        &mut fake,
        "schedule",
        None,
        1,
        &[listed(3000, "schedule", "main")],
    );
    for id in [1000, 1001, 2000, 2001, 3000] {
        run(&mut fake, id);
        jobs(
            &mut fake,
            id,
            1,
            1,
            &[job("only", Some("2026-11-01T10:01:10Z"))],
        );
    }
    let histories = gather(&fake, &repo(), "main", 2, None).unwrap_or_else(|e| unreachable!("{e}"));
    let sizes: Vec<(Population, Vec<u64>)> = histories
        .iter()
        .map(|history| {
            (
                history.population,
                history.samples.iter().map(|sample| sample.run_id).collect(),
            )
        })
        .collect();
    assert_eq!(
        sizes,
        vec![
            (Population::PullRequest, vec![1000, 1001]),
            (Population::MainPush, vec![2000, 2001]),
            (Population::Schedule, vec![3000]),
        ]
    );
}

#[test]
fn a_slow_run_is_judged_by_its_own_wall_time_not_by_the_run_before_it() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[(NORMAL, 115, "success"), (QUICK, 99, "success")],
    );
    let running = executing(&mut fake, SLOW, "push", 226);
    let evaluation = judged(&fake, &running);
    assert!(!evaluation.passed(), "{evaluation:?}");
    assert_eq!(evaluation.breaches.len(), 1, "{:?}", evaluation.breaches);
    assert_eq!(evaluation.breaches[0].value, 226);
    assert_eq!(evaluation.breaches[0].population, Population::MainPush);
}

#[test]
fn a_run_that_failed_the_budget_is_not_a_sample_for_the_next_run() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[
            (SLOW, 226, "failure"),
            (NORMAL, 115, "success"),
            (QUICK, 99, "success"),
        ],
    );
    let running = executing(&mut fake, REVERT, "push", 93);
    let histories =
        gather(&fake, &repo(), "main", 10, Some(&running)).unwrap_or_else(|e| unreachable!("{e}"));
    let ids: Vec<u64> = histories[0].samples.iter().map(|s| s.run_id).collect();
    assert_eq!(ids, [REVERT, NORMAL, QUICK]);
    assert!(judged(&fake, &running).passed());
}

#[test]
fn a_manual_run_has_no_population_so_only_the_recorded_runs_are_judged() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[(NORMAL, 115, "success"), (QUICK, 99, "success")],
    );
    let running = executing(&mut fake, SLOW, "workflow_dispatch", 226);
    assert_eq!(running.population, None);
    let ids: Vec<u64> = gather(&fake, &repo(), "main", 10, Some(&running))
        .unwrap_or_else(|e| unreachable!("{e}"))[0]
        .samples
        .iter()
        .map(|s| s.run_id)
        .collect();
    assert_eq!(ids, [NORMAL, QUICK]);
}
