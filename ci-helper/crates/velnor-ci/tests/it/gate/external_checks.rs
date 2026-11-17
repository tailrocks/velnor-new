// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::assert_refused;
use super::pr_only::{CHECK_RULESET, aggregate_path, check_ruleset};
use super::pr_only::{allowed, checks_path, protected, suites_path};

#[test]
fn unfinished_suites_from_a_possible_exempt_app_cannot_prove_absence() {
    for status in ["queued", "in_progress"] {
        let pending = || {
            protected().set(suites_path(1), json!({"total_count": 1, "check_suites": [{"id": 9, "status": status, "app": {"id": 343_649}}]}))
        };
        assert_refused(&allowed(&pending()), "unfinished check suite 9");
        for app in [json!(-1), Value::Null] {
            let mut detail = check_ruleset();
            detail["rules"][0]["parameters"]["required_status_checks"][1]["integration_id"] =
                app.clone();
            let mut rule = super::required(&[
                super::check("Required"),
                json!({"context": "DCO", "integration_id": app}),
            ]);
            rule["ruleset_id"] = json!(24_396_608);
            let recorded = pending()
                .rules(vec![super::pr_only::pr_rule(), rule])
                .set(CHECK_RULESET.to_owned(), detail);
            assert_refused(&allowed(&recorded), "unfinished check suite 9");
        }
        let unrelated = protected().set(suites_path(1), json!({"total_count": 1, "check_suites": [{"id": 9, "status": status, "app": {"id": 999}}]}));
        assert!(allowed(&unrelated).is_ok_and(|verdict| verdict.ready));
    }
    assert!(allowed(&protected()).is_ok_and(|verdict| verdict.ready));
    for suite in [
        json!({"id": 9, "app": {"id": 343_649}}),
        json!({"id": 9, "status": "completed"}),
    ] {
        assert!(
            allowed(&protected().set(
                suites_path(1),
                json!({"total_count": 1, "check_suites": [suite]})
            ))
            .is_err()
        );
    }
}

#[test]
fn external_check_runs_prevent_pr_only_exemptions_regardless_of_result_or_app() {
    assert!(allowed(&protected()).is_ok_and(|verdict| verdict.ready));
    for (name, status, conclusion, app) in [
        ("DCO", "completed", json!("failure"), 343_649),
        ("DCO", "completed", json!("success"), 999),
        ("dco", "completed", json!("skipped"), 999),
        ("DCO", "queued", Value::Null, 15368),
    ] {
        let recorded = protected().set(
            checks_path(1),
            json!({"total_count": 1, "check_runs": [{"name": name, "head_sha": super::GREEN, "status": status, "conclusion": conclusion, "app": {"id": app}}]}),
        );
        assert_refused(&allowed(&recorded), "PR-only check DCO has");
    }
}

#[test]
fn aggregate_external_check_runs_prevent_pr_only_exemptions_on_every_page() {
    for (page, name) in [(1, "DCO"), (2, "dco")] {
        let recorded = |name: &str| {
            let mut recorded = protected();
            if page == 2 {
                recorded = recorded.set(
                    aggregate_path(1),
                    json!({"total_count": 2, "check_runs": [{"name": "unrelated", "check_suite": {"id": 9}}]}),
                );
            }
            recorded.set(
                aggregate_path(page),
                json!({"total_count": page, "check_runs": [{"name": name, "head_sha": super::GREEN, "status": "completed", "conclusion": "failure", "app": {"id": 343_649}, "check_suite": {"id": 9}}]}),
            )
        };
        assert!(allowed(&recorded("unrelated")).is_ok_and(|verdict| verdict.ready));
        assert_refused(&allowed(&recorded(name)), "PR-only check DCO has");
    }
}

#[test]
fn all_external_check_pages_are_required_even_for_historical_or_incomplete_results() {
    for body in [
        json!({"total_count": 1, "check_runs": [{"name": "unrelated"}]}),
        json!({"total_count": 2, "check_runs": []}),
    ] {
        assert!(allowed(&protected().set(aggregate_path(1), body)).is_err());
    }
    let first = json!({"total_count": 2, "check_runs": [{"name": "unrelated"} ]});
    let recorded = || protected().set(checks_path(1), first.clone());
    let historical = recorded().set(
        checks_path(2),
        json!({"total_count": 2, "check_runs": [{"name": "DCO", "status": "completed", "conclusion": "failure"}]}),
    );
    assert_refused(&allowed(&historical), "PR-only check DCO has");
    assert_refused(&allowed(&recorded()), "no recorded response");
    for body in [
        json!({"total_count": 2, "check_runs": []}),
        json!({"total_count": 2, "check_runs": [{}]}),
        json!({"check_runs": []}),
        json!({"total_count": 0, "check_runs": []}),
    ] {
        assert!(allowed(&recorded().set(checks_path(2), body)).is_err());
    }
    let suites = || {
        protected().set(
            suites_path(1),
            json!({"total_count": 2, "check_suites": [{"id": 9, "status": "completed", "app": {"id": 343_649}}]}),
        )
    };
    assert_refused(&allowed(&suites()), "no recorded response");
    for body in [
        json!({"total_count": 2, "check_suites": []}),
        json!({"total_count": 2, "check_suites": [{}]}),
        json!({"check_suites": []}),
        json!({"total_count": 0, "check_suites": []}),
    ] {
        assert!(allowed(&suites().set(suites_path(2), body)).is_err());
    }
}

#[test]
fn an_external_check_in_a_suite_older_than_the_newest_thousand_prevents_exemption() {
    let history = |dco: bool| {
        let mut recorded = protected();
        let mut ids: Vec<u64> = (1..=1001).rev().collect();
        for (page, ids) in (1..).zip(ids[..1000].chunks(100)) {
            let runs: Vec<Value> = ids
                .iter()
                .rev()
                .map(|id| json!({"name": "unrelated", "check_suite": {"id": id}}))
                .collect();
            recorded = recorded.set(
                aggregate_path(page),
                json!({"total_count": 1000, "check_runs": runs}),
            );
        }
        ids.rotate_right(1);
        for (page, ids) in (1..).zip(ids.chunks(100)) {
            let suites: Vec<Value> = ids
                .iter()
                .map(|id| json!({"id": id, "status": "completed", "app": {"id": 343_649}}))
                .collect();
            recorded = recorded.set(
                suites_path(page),
                json!({"total_count": 1001, "check_suites": suites}),
            );
        }
        for id in ids {
            let name = if id == 1 && dco { "DCO" } else { "unrelated" };
            recorded = recorded.set(
                format!("/repos/tailrocks/velnor-new/check-suites/{id}/check-runs?filter=all&per_page=100&page=1"),
                json!({"total_count": 1, "check_runs": [{"name": name, "status": "completed", "conclusion": "failure"}]}),
            );
        }
        recorded
    };
    assert!(
        history(false)
            .gate_with_budget(100)
            .is_ok_and(|verdict| verdict.ready)
    );
    assert_refused(
        &history(true).gate_with_budget(100),
        "PR-only check DCO has",
    );
}

#[test]
fn unrelated_external_checks_do_not_block_or_replace_required_push_jobs() {
    let recorded = || {
        protected().set(
            checks_path(1),
            json!({"total_count": 1, "check_runs": [{"name": "Required", "conclusion": "success"}]}),
        )
    };
    assert!(allowed(&recorded()).is_ok_and(|verdict| verdict.ready));
    assert_refused(
        &allowed(&recorded().green_job("Required", |job| job["name"] = json!("Other"))),
        "no job of a push run on main reports it",
    );
}
