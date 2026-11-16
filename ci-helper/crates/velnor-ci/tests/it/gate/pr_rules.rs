// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::rules_path;

use super::pr_only::{RULESET, allowed, pr_rule, protected, ruleset};
use super::{assert_refused, check, required};
use crate::support::repo;

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
        let recorded = protected().rules(vec![rule, required(&[check("Required"), check("DCO")])]);
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
    rules.push(required(&[check("Required"), check("DCO")]));
    let recorded = protected()
        .rules(rules)
        .set(rules_path(&repo(), "main", 2), json!([inherited]))
        .set(RULESET.to_owned(), detail);
    assert!(allowed(&recorded).is_ok_and(|verdict| verdict.ready));
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
            required(&[check("Required"), check("DCO")]),
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
        .rules(vec![rule, required(&[check("Required"), check("DCO")])])
        .set(RULESET.to_owned(), detail);
    assert!(allowed(&unknown).is_err());
}
