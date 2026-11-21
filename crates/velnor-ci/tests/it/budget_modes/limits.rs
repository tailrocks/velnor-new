use super::*;

#[test]
fn a_mode_without_its_own_limit_uses_the_plain_key_and_zero_switches_it_off() {
    let mut limits = two_modes();
    limits.main_wall_seconds = 500;
    limits.runner_seconds = 500;
    limits.none = ModeBudget {
        main_wall_seconds: Some(0),
        ..ModeBudget::default()
    };
    let nothing = history_of(
        Population::MainPush,
        Selection::Nothing,
        vec![sample(7, 600, 400, None)],
    );
    let evaluation = evaluate(&limits, &[nothing]);
    assert!(
        evaluation.passed(),
        "the none table switched the wall check off: {evaluation:?}"
    );
    let nothing_slow_runner = history_of(
        Population::MainPush,
        Selection::Nothing,
        vec![sample(8, 600, 700, None)],
    );
    let evaluation = evaluate(&limits, &[nothing_slow_runner]);
    assert_eq!(
        breached(&evaluation),
        [(Some(Selection::Nothing), 700, 500)],
        "the runner limit falls back to the plain key"
    );
}

#[test]
fn a_limit_in_a_mode_table_alone_enables_the_budget_and_names_missing_runs() {
    let only_full = SpeedBudget {
        full: ModeBudget {
            main_wall_seconds: Some(900),
            ..ModeBudget::default()
        },
        ..SpeedBudget::default()
    };
    assert!(only_full.enabled());
    let evaluation = evaluate(&only_full, &[]);
    assert_eq!(
        evaluation.missing,
        ["the generated workflow has no successful run to measure"]
    );
}

#[test]
fn an_affected_run_is_not_judged_against_the_full_runs_before_it() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1, 2, 3, 4]);
    for id in [1, 2] {
        finished(&mut fake, id, 800, Some("Speed budget (full)"));
    }
    for id in [3, 4] {
        finished(&mut fake, id, 70, Some("Speed budget (affected)"));
    }
    let running = executing(&mut fake, AFFECTED_RUN, 80, Some("Speed budget (affected)"));
    let (ids, evaluation) = judged(&fake, &running, &two_modes());
    assert_eq!(ids, [AFFECTED_RUN, 3, 4]);
    assert!(evaluation.passed(), "{evaluation:?}");
    let slow = executing(
        &mut fake,
        AFFECTED_RUN + 1,
        140,
        Some("Speed budget (affected)"),
    );
    let (_, evaluation) = judged(&fake, &slow, &two_modes());
    assert_eq!(
        breached(&evaluation).first(),
        Some(&(Some(Selection::Affected), 140, 100)),
        "{evaluation:?}"
    );
}

#[test]
fn the_per_mode_tables_of_the_budget_are_read_with_the_plain_keys_as_fallback() {
    let config = b"[budget]\nmain_wall_seconds = 700\nrunner_seconds = 800\n\n[budget.full]\nmain_wall_seconds = 900\n\n[budget.affected]\nmain_wall_seconds = 100\nrunner_seconds = 0\n";
    let parsed = velnor_rust::budget(config)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .unwrap_or_else(|| unreachable!("the table is missing"));
    assert_eq!(parsed.main_wall_seconds, 700);
    assert_eq!(parsed.full.main_wall_seconds, Some(900));
    assert_eq!(parsed.affected.runner_seconds, Some(0));
    assert!(!parsed.none.is_set());
    let limits = parsed.for_mode(Some(Selection::Full));
    assert_eq!(
        (limits.main_wall_seconds, limits.runner_seconds),
        (900, 800)
    );
    let limits = parsed.for_mode(Some(Selection::Affected));
    assert_eq!((limits.main_wall_seconds, limits.runner_seconds), (100, 0));
    let limits = parsed.for_mode(Some(Selection::Nothing));
    assert_eq!(
        (limits.main_wall_seconds, limits.runner_seconds),
        (700, 800)
    );
    let limits = parsed.for_mode(None);
    assert_eq!(
        (limits.main_wall_seconds, limits.runner_seconds),
        (700, 800)
    );
}

#[test]
fn the_title_of_the_budget_job_names_the_selection_and_nothing_else_does() {
    for selection in Selection::ALL {
        let title = format!("Speed budget ({})", selection.name());
        assert_eq!(Selection::of_job_title(&title), Some(selection));
    }
    for title in [
        "Speed budget",
        "Speed budget (unknown)",
        "Speed budget (everything)",
        "rust (full)",
        "Speed budget (full) ",
    ] {
        assert_eq!(Selection::of_job_title(title), None, "{title}");
    }
}
