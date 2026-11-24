use super::*;

#[test]
fn a_page_that_ends_before_the_job_count_is_refused_not_summed() {
    let mut fake = FakeGitHub::default();
    run(&mut fake, 7);
    let page: Vec<Value> = (0..100)
        .map(|n| job(&format!("job-{n}"), Some("2026-11-01T10:01:10Z")))
        .collect();
    jobs(&mut fake, 7, 1, 150, &page);
    jobs(&mut fake, 7, 2, 150, &[]);
    let error = sample(&fake, &repo(), 7).expect_err("a partial job list was accepted");
    assert!(
        error.to_string().contains("refusing incomplete jobs data"),
        "{error}"
    );
}

#[test]
fn complete_runs_are_summed_per_job() {
    let mut fake = FakeGitHub::default();
    run(&mut fake, 7);
    jobs(
        &mut fake,
        7,
        1,
        2,
        &[
            job("a", Some("2026-11-01T10:01:10Z")),
            job("b", Some("2026-11-01T10:02:10Z")),
        ],
    );
    let measured = sample(&fake, &repo(), 7).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(measured.runner_seconds, 60 + 120);
    assert_eq!(measured.wall_seconds, 300);
}

#[test]
fn a_history_with_no_scheduled_runs_is_read_for_a_bounded_number_of_pages() {
    let mut fake = FakeGitHub::default();
    let full = |event: &str, branch: &str, first: u64| -> Vec<Value> {
        (0..100)
            .map(|n| {
                let mut run = listed(first + n, event, branch);
                run["run_attempt"] = json!(2);
                run
            })
            .collect()
    };
    for page in 1..=10_u32 {
        let first = u64::from(page) * 1000;
        listing(
            &mut fake,
            "pull_request",
            None,
            page,
            &full("pull_request", "feature", first),
        );
        listing(
            &mut fake,
            "push",
            Some("main"),
            page,
            &full("push", "main", first),
        );
        listing(
            &mut fake,
            "schedule",
            None,
            page,
            &full("schedule", "main", first),
        );
    }
    let histories = gather(&fake, &repo(), "main", 2, None).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(histories.is_empty(), "{histories:?}");
}

#[test]
fn the_run_after_a_breach_passes_when_it_is_back_to_normal() {
    let mut fake = FakeGitHub::default();
    // The slow run passed the old check, so it is listed as a success.
    pushes(
        &mut fake,
        &[
            (SLOW, 226, "success"),
            (NORMAL, 115, "success"),
            (QUICK, 99, "success"),
        ],
    );
    let running = executing(&mut fake, REVERT, "push", 93);
    let evaluation = judged(&fake, &running);
    assert!(evaluation.passed(), "{evaluation:?}");
}

#[test]
fn the_running_run_is_never_counted_twice_and_the_window_stays_at_the_last_runs() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[
            (REVERT, 93, "success"),
            (NORMAL, 115, "success"),
            (QUICK, 99, "success"),
        ],
    );
    let running = executing(&mut fake, REVERT, "push", 93);
    let histories =
        gather(&fake, &repo(), "main", 2, Some(&running)).unwrap_or_else(|e| unreachable!("{e}"));
    let ids: Vec<u64> = histories[0].samples.iter().map(|s| s.run_id).collect();
    assert_eq!(ids, [REVERT, NORMAL]);
}
