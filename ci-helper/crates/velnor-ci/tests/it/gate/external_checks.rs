// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::assert_refused;
use super::pr_only::{allowed, checks_path, protected, suites_path};

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
fn all_external_check_pages_are_required_even_for_historical_or_incomplete_results() {
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
            json!({"total_count": 2, "check_suites": [{"id": 9}]}),
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
        let mut recorded = protected().set(
            format!(
                "/repos/tailrocks/velnor-new/commits/{}/check-runs?filter=all&per_page=100&page=1",
                super::GREEN
            ),
            json!({"total_count": 1, "check_runs": [{"name": "unrelated"}]}),
        );
        let ids: Vec<u64> = (1..=1001).rev().collect();
        for (page, ids) in (1..).zip(ids.chunks(100)) {
            let suites: Vec<Value> = ids.iter().map(|id| json!({"id": id})).collect();
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
    assert!(allowed(&history(false)).is_ok_and(|verdict| verdict.ready));
    assert_refused(&allowed(&history(true)), "PR-only check DCO has");
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
