// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::rules_path;

use super::pr_only::{CHECK_RULESET, check_ruleset, dco};
use super::pr_only::{RULESET, allowed, pr_rule, protected, ruleset};
use super::{assert_refused, check, required};
use crate::support::repo;

#[test]
fn safe_check_enforcement_must_cover_each_exempt_app_binding() {
    for (unsafe_app, safe_app, accepted) in [
        (json!(123), json!(456), false),
        (json!(123), json!(-1), false),
        (json!(123), Value::Null, false),
        (json!(123), json!(123), true),
        (json!(-1), json!(123), true),
    ] {
        let binding = |app: Value| json!({"context": "DCO", "integration_id": app});
        let safe = required(&[check("Required"), binding(safe_app)]);
        let mut unsafe_rule = required(&[binding(unsafe_app)]);
        unsafe_rule["ruleset_id"] = json!(8);
        let mut safe_detail = check_ruleset();
        safe_detail["rules"] = json!([safe.clone()]);
        let mut unsafe_detail = check_ruleset();
        unsafe_detail["id"] = json!(8);
        unsafe_detail["rules"] = json!([unsafe_rule.clone()]);
        unsafe_detail["bypass_actors"] = json!([{"actor_id": 1}]);
        let recorded = protected()
            .rules(vec![pr_rule(), unsafe_rule, safe])
            .set(CHECK_RULESET.to_owned(), safe_detail)
            .set(
                "/repos/tailrocks/velnor-new/rulesets/8".to_owned(),
                unsafe_detail,
            );
        if accepted {
            assert!(allowed(&recorded).is_ok_and(|verdict| verdict.ready));
        } else {
            assert_refused(
                &allowed(&recorded),
                "no verified non-bypassable required_status_checks ruleset",
            );
        }
    }
}

#[test]
fn each_exempt_context_needs_a_non_bypassable_required_check_owner() {
    assert!(allowed(&protected()).is_ok_and(|verdict| verdict.ready));
    for (key, value) in [
        (
            "bypass_actors",
            json!([{"actor_id": 1, "bypass_mode": "always"}]),
        ),
        ("bypass_actors", Value::Null),
        ("rules", json!([{ "type": "pull_request" }])),
        ("id", json!(8)),
        ("source", json!("other/repo")),
        ("source_type", json!("Organization")),
        ("target", json!("tag")),
        ("enforcement", json!("evaluate")),
    ] {
        let mut detail = check_ruleset();
        detail[key] = value;
        assert_refused(
            &allowed(&protected().set(CHECK_RULESET.to_owned(), detail)),
            "no verified non-bypassable required_status_checks ruleset for PR-only check DCO",
        );
    }
    let classic = protected().rules(vec![pr_rule()]).protection(
        "main",
        "everyone",
        &["Required", "DCO"],
        &json!([]),
    );
    assert_refused(
        &allowed(&classic),
        "no verified non-bypassable required_status_checks ruleset",
    );
    let mut unsafe_rule = required(&[dco()]);
    unsafe_rule["ruleset_id"] = json!(8);
    let mut detail = check_ruleset();
    detail["id"] = json!(8);
    detail["bypass_actors"] = json!([{"actor_id": 1}]);
    let duplicates = protected()
        .rules(vec![
            pr_rule(),
            unsafe_rule,
            required(&[check("Required"), dco()]),
        ])
        .set("/repos/tailrocks/velnor-new/rulesets/8".to_owned(), detail);
    assert!(allowed(&duplicates).is_ok_and(|verdict| verdict.ready));
}

#[test]
fn bypassable_or_unverifiable_pull_request_rules_cannot_authorize_exemptions() {
    assert!(allowed(&protected()).is_ok_and(|verdict| verdict.ready));
    for mode in ["always", "pull_request"] {
        let mut detail = ruleset();
        detail["bypass_actors"] =
            json!([{"actor_id": 1, "actor_type": "Integration", "bypass_mode": mode}]);
        assert_refused(
            &allowed(&protected().set(RULESET.to_owned(), detail)),
            "no verified non-bypassable pull_request ruleset",
        );
    }
    for (key, value) in [
        ("bypass_actors", Value::Null),
        ("id", json!(8)),
        ("source", json!("another/repo")),
        ("source_type", json!("Organization")),
        ("target", json!("tag")),
        ("enforcement", json!("evaluate")),
        ("rules", json!([])),
    ] {
        let mut detail = ruleset();
        detail[key] = value;
        assert!(
            allowed(&protected().set(RULESET.to_owned(), detail)).is_err(),
            "{key}"
        );
    }
    let mut hidden = ruleset();
    hidden
        .as_object_mut()
        .unwrap_or_else(|| unreachable!())
        .remove("bypass_actors");
    assert!(allowed(&protected().set(RULESET.to_owned(), hidden)).is_err());
    for key in ["ruleset_id", "ruleset_source", "ruleset_source_type"] {
        let mut rule = pr_rule();
        rule[key] = Value::Null;
        let recorded = protected().rules(vec![rule, required(&[check("Required"), dco()])]);
        assert!(allowed(&recorded).is_err(), "{key}");
    }
}

#[test]
fn an_inherited_non_bypassable_ruleset_on_a_later_page_enforces_pull_requests() {
    let mut inherited = pr_rule();
    inherited["ruleset_source_type"] = json!("Organization");
    inherited["ruleset_source"] = json!("tailrocks");
    let mut detail = ruleset();
    detail["source_type"] = json!("Organization");
    detail["source"] = json!("tailrocks");
    let mut rules = vec![json!({"type": "deletion"}); 99];
    rules.push(required(&[check("Required"), dco()]));
    let recorded = protected()
        .rules(rules)
        .set(rules_path(&repo(), "main", 2), json!([inherited]))
        .set(RULESET.to_owned(), detail);
    assert!(allowed(&recorded).is_ok_and(|verdict| verdict.ready));
}

#[test]
fn inaccessible_inherited_rulesets_name_the_permission_or_plan_needed() {
    struct Forbidden(super::Recorded);
    impl velnor_ci::github::GitHub for Forbidden {
        fn get(&self, path: &str) -> Result<String, velnor_ci::error::CiError> {
            if path == RULESET {
                return Err(velnor_ci::error::CiError::new(
                    "GET ruleset: HTTP 403 Forbidden",
                ));
            }
            self.0
                .0
                .get(path)
                .map(Value::to_string)
                .ok_or_else(|| velnor_ci::error::CiError::new("missing response"))
        }
    }
    for source_type in ["Organization", "Enterprise"] {
        let mut rule = pr_rule();
        rule["ruleset_source_type"] = json!(source_type);
        rule["ruleset_source"] = json!("tailrocks");
        let recorded = protected().rules(vec![rule, required(&[check("Required"), dco()])]);
        let said = velnor_ci::gate::release_gate(
            &Forbidden(recorded),
            &repo(),
            super::GREEN,
            &["DCO".to_owned()],
            None,
        )
        .map_err(|e| e.to_string());
        assert_refused(
            &said,
            &format!("cannot verify {source_type} ruleset 7 from tailrocks"),
        );
        assert_refused(
            &said,
            "permission and a GitHub plan that supports reading inherited rulesets",
        );
        assert_refused(&said, "HTTP 403 Forbidden");
    }
    assert!(allowed(&protected()).is_ok_and(|verdict| verdict.ready));
}

#[test]
fn one_non_bypassable_ruleset_suffices_but_unknown_sources_do_not() {
    let mut other = pr_rule();
    other["ruleset_id"] = json!(8);
    let mut unsafe_detail = ruleset();
    unsafe_detail["bypass_actors"] = json!([{"actor_id": 1, "bypass_mode": "always"}]);
    let mut safe_detail = ruleset();
    safe_detail["id"] = json!(8);
    let recorded = protected()
        .rules(vec![
            pr_rule(),
            other,
            required(&[check("Required"), dco()]),
        ])
        .set(RULESET.to_owned(), unsafe_detail)
        .set(
            "/repos/tailrocks/velnor-new/rulesets/8".to_owned(),
            safe_detail,
        );
    assert!(allowed(&recorded).is_ok_and(|verdict| verdict.ready));
    let mut rule = pr_rule();
    rule["ruleset_source_type"] = json!("Unknown");
    let mut detail = ruleset();
    detail["source_type"] = json!("Unknown");
    let unknown = protected()
        .rules(vec![rule, required(&[check("Required"), dco()])])
        .set(RULESET.to_owned(), detail);
    assert!(allowed(&unknown).is_err());
}
