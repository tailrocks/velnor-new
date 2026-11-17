// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::run_path;

use super::pr_only::{
    CHECK_RULESET, aggregate_path, check_ruleset, checks_path, pr_rule, protected, suites_path,
};
use super::{GREEN, Recorded, assert_refused, check, required};
use crate::support::repo;

fn actions_bound(app: Value) -> Recorded {
    let binding = json!({"context": "DCO", "integration_id": app});
    let rule = required(&[check("Required"), binding]);
    let mut detail = check_ruleset();
    detail["rules"] = json!([rule.clone()]);
    protected()
        .rules(vec![pr_rule(), rule])
        .set(CHECK_RULESET.to_owned(), detail)
        .set(run_path(&repo(), 77), json!({"check_suite_id": 9}))
        .set(suites_path(1), json!({"total_count": 1, "check_suites": [{"id": 9, "status": "in_progress", "app": {"id": 15368}}]}))
}

#[test]
fn only_the_current_runs_unfinished_suite_is_ignored() {
    for app in [json!(15368), json!(-1), Value::Null] {
        let own = actions_bound(app);
        let gate = |recorded: &Recorded| recorded.gate_in_run(GREEN, &["DCO".to_owned()], Some(77));
        assert!(gate(&own).is_ok_and(|verdict| verdict.ready));
        let other = own.set(
            suites_path(1),
            json!({"total_count": 2, "check_suites": [
                {"id": 9, "status": "in_progress", "app": {"id": 15368}},
                {"id": 10, "status": "in_progress", "app": {"id": 15368}}
            ]}),
        );
        assert_refused(&gate(&other), "unfinished check suite 10");
    }
}

#[test]
fn current_run_identity_must_be_resolved_before_ignoring_a_suite() {
    for response in [json!({}), json!({"check_suite_id": null})] {
        let own = actions_bound(json!(15368)).set(run_path(&repo(), 77), response);
        assert_refused(
            &own.gate_in_run(GREEN, &["DCO".to_owned()], Some(77)),
            "current workflow run",
        );
    }
    let own = actions_bound(json!(15368));
    assert_refused(
        &own.gate_in_run(GREEN, &["DCO".to_owned()], Some(78)),
        "no recorded response",
    );
    assert_refused(
        &own.gate_in_run(GREEN, &["DCO".to_owned()], None),
        "unfinished check suite 9",
    );
}

#[test]
fn current_suite_check_names_still_disprove_pr_only_absence() {
    for path in [aggregate_path(1), checks_path(1)] {
        let own = actions_bound(json!(15368)).set(path, json!({"total_count": 1, "check_runs": [{"name": "DCO", "app": {"id": 15368}, "check_suite": {"id": 9}}]}));
        assert_refused(
            &own.gate_in_run(GREEN, &["DCO".to_owned()], Some(77)),
            "PR-only check DCO has",
        );
    }
}

#[test]
fn a_changed_listing_refuses_with_a_retry_diagnosis() {
    let recorded = protected()
        .set(suites_path(1), json!({"total_count": 2, "check_suites": [{"id": 9, "status": "completed", "app": {"id": 343649}}]}))
        .set(suites_path(2), json!({"total_count": 3, "check_suites": []}));
    assert_refused(
        &super::pr_only::allowed(&recorded),
        "check suites listing changed during pagination; retry",
    );
}
