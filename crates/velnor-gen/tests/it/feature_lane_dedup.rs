// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::feature_lanes::{jobs_of, steps_named};

#[test]
fn lanes_with_the_same_features_share_one_lint_step_and_one_doc_step() -> TestResult {
    let lane = |features: &str, filter: &str, lint: bool| {
        format!(
            "[[test_lane]]\nroot = '.'\nfeatures = [{features}]\npackages = ['package-0']\nfilter = '{filter}'\nlint = {lint}\ndoc = {lint}\n"
        )
    };
    let config = format!(
        "{}{}{}",
        lane("'a', 'b'", "test(~one)", true),
        lane("'b', 'a'", "test(~two)", true),
        lane("'c'", "test(~three)", true),
    );
    let jobs = jobs_of(&config)?;
    for (name, count) in [
        ("Test (features: a,b)", 1),
        ("Test (features: b,a)", 1),
        ("Test (features: c)", 1),
        ("Lint (features: a,b)", 1),
        ("Lint (features: b,a)", 0),
        ("Lint (features: c)", 1),
        ("Documentation (features: a,b)", 1),
        ("Documentation (features: b,a)", 0),
        ("Documentation (features: c)", 1),
    ] {
        assert_eq!(steps_named(&jobs, name).len(), count, "{name}");
    }
    let off_first = format!(
        "{}{}",
        lane("'a'", "test(~one)", false),
        lane("'a'", "test(~two)", true)
    );
    let jobs = jobs_of(&off_first)?;
    assert_eq!(steps_named(&jobs, "Lint (features: a)").len(), 1);
    assert_eq!(steps_named(&jobs, "Documentation (features: a)").len(), 1);
    Ok(())
}
