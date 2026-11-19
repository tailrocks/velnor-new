// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::budget::{Population, evaluate};

use crate::budget::{budget, history, sample};

#[test]
fn a_run_without_a_setup_measurement_leaves_the_window_missing_and_not_a_shorter_median() {
    let histories = vec![history(
        Population::MainPush,
        vec![
            sample(3, 100, 100, Some(10)),
            sample(2, 100, 100, None),
            sample(1, 100, 100, Some(10)),
        ],
    )];
    let evaluation = evaluate(&budget(0, 500, 0, 30), &histories);
    assert!(evaluation.breaches.is_empty(), "{:?}", evaluation.breaches);
    assert_eq!(
        evaluation.missing,
        ["the default branch setup seconds of the measured runs is not measured"]
    );
    assert!(!evaluation.passed());
    let complete = vec![history(
        Population::MainPush,
        vec![
            sample(3, 100, 100, Some(10)),
            sample(2, 100, 100, Some(12)),
            sample(1, 100, 100, Some(10)),
        ],
    )];
    assert!(evaluate(&budget(0, 500, 0, 30), &complete).passed());
}
