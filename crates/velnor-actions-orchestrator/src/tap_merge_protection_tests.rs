use super::rulesets::ruleset_detail_route;
use super::*;

fn facts() -> ProtectionFacts {
    ProtectionFacts {
        strict: true,
        checks: vec![("CI".to_owned(), 17)],
        writer_bypass: false,
    }
}

fn run(status: &str, conclusion: &str, sha: &str) -> Value {
    named_run("CI", 17, status, conclusion, sha)
}

fn named_run(name: &str, app_id: u64, status: &str, conclusion: &str, sha: &str) -> Value {
    serde_json::json!({
        "name": name,
        "app": {"id": app_id},
        "head_sha": sha,
        "status": status,
        "conclusion": if conclusion.is_empty() { Value::Null } else { Value::String(conclusion.to_owned()) }
    })
}

#[test]
fn required_check_requires_exact_app_head_and_terminal_success() {
    let checks = exact_required_checks(
        &facts(),
        &[run("completed", "success", &"a".repeat(40))],
        &"a".repeat(40),
    )
    .expect("check lookup")
    .expect("ready");
    assert_eq!(checks[0].required_app_id, Some(17));
    assert!(checks[0].completed_successfully);
}

#[test]
fn pending_check_waits_but_failure_and_wrong_head_reject() {
    assert!(
        exact_required_checks(&facts(), &[], &"a".repeat(40))
            .expect("pending")
            .is_none()
    );
    assert!(
        exact_required_checks(
            &facts(),
            &[run("in_progress", "", &"a".repeat(40))],
            &"a".repeat(40)
        )
        .expect("pending")
        .is_none()
    );
    assert!(
        exact_required_checks(
            &facts(),
            &[run("completed", "failure", &"a".repeat(40))],
            &"a".repeat(40)
        )
        .is_err()
    );
    assert!(
        exact_required_checks(
            &facts(),
            &[run("completed", "success", &"b".repeat(40))],
            &"a".repeat(40)
        )
        .is_err()
    );
}

#[test]
fn pending_earlier_check_does_not_hide_later_terminal_failure() {
    let facts = ProtectionFacts {
        strict: true,
        checks: vec![("Pending".to_owned(), 17), ("Failed".to_owned(), 18)],
        writer_bypass: false,
    };
    let runs = [
        named_run("Pending", 17, "in_progress", "", &"a".repeat(40)),
        named_run("Failed", 18, "completed", "failure", &"a".repeat(40)),
    ];
    assert!(exact_required_checks(&facts, &runs, &"a".repeat(40)).is_err());
}

#[test]
fn branch_api_requires_app_bound_checks_and_dco() {
    let json = serde_json::json!({
        "enforce_admins": {"enabled": true},
        "required_pull_request_reviews": {"bypass_pull_request_allowances": {
            "users": [], "teams": [], "apps": []
        }},
        "required_status_checks": {
            "strict": true,
            "checks": [
                {"context": "CI", "app_id": 17},
                {"context": "DCO", "app_id": 19}
            ]
        }
    });
    let classic = parse_classic_protection(&json, 17, "tap-bot[bot]").expect("classic protection");
    assert_eq!(classic.checks.len(), 2);
    assert!(classic.strict);
    assert!(!classic.writer_bypass);
    let missing_app = serde_json::json!({
        "enforce_admins": {"enabled": true},
        "required_pull_request_reviews": {"bypass_pull_request_allowances": {
            "users": [], "teams": [], "apps": []
        }},
        "required_status_checks": {"strict": true, "checks": [{"context": "CI", "app_id": null}]}
    });
    assert!(parse_classic_protection(&missing_app, 17, "tap-bot[bot]").is_err());
    assert!(
        parse_classic_protection(
            &serde_json::json!({
                "enforce_admins": {"enabled": false},
                "required_pull_request_reviews": {"bypass_pull_request_allowances": {
                    "users": [], "teams": [], "apps": []
                }}
            }),
            17,
            "tap-bot[bot]"
        )
        .is_err()
    );
    let bypasses_writer = serde_json::json!({
        "enforce_admins": {"enabled": true},
        "required_pull_request_reviews": {"bypass_pull_request_allowances": {
            "users": [{"login": "tap-bot[bot]"}], "teams": [], "apps": [{"id": 17}]
        }}
    });
    let classic =
        parse_classic_protection(&bypasses_writer, 17, "tap-bot[bot]").expect("read bypasses");
    assert!(classic.writer_bypass);
}

#[test]
fn ruleset_writer_bypass_and_non_active_state_are_rejected_or_detected() {
    let value = serde_json::json!({
        "target": "branch",
        "enforcement": "active",
        "bypass_actors": [{"actor_type": "Integration", "actor_id": 17}],
        "rules": [{"type": "required_status_checks", "parameters": {
            "strict_required_status_checks_policy": true,
            "required_status_checks": [{"context": "DCO", "integration_id": 18}]
        }}]
    });
    let ruleset = parse_ruleset(&value, 17, 29).expect("ruleset");
    assert_eq!(ruleset.checks, vec![("DCO".to_owned(), 18)]);
    assert!(ruleset.writer_bypass);
    assert!(ruleset.strict);
    let mut inactive = value;
    inactive["enforcement"] = serde_json::json!("evaluate");
    assert!(parse_ruleset(&inactive, 17, 29).is_err());
}

#[test]
fn ruleset_bypass_rejects_unresolved_actors_and_detects_writer_user_id() {
    for kind in ["Team", "OrganizationAdmin", "DeployKey"] {
        let value = ruleset_with_bypass(&serde_json::json!({
            "actor_type": kind,
            "actor_id": 41
        }));
        assert!(parse_ruleset(&value, 17, 29).is_err(), "{kind}");
    }

    let writer_user = ruleset_with_bypass(&serde_json::json!({
        "actor_type": "User",
        "actor_id": 29
    }));
    assert!(
        parse_ruleset(&writer_user, 17, 29)
            .expect("writer user bypass identity is known")
            .writer_bypass
    );

    let other_user = ruleset_with_bypass(&serde_json::json!({
        "actor_type": "User",
        "actor_id": 30
    }));
    assert!(
        !parse_ruleset(&other_user, 17, 29)
            .expect("different user identity")
            .writer_bypass
    );
}

fn ruleset_with_bypass(actor: &Value) -> Value {
    serde_json::json!({
        "target": "branch",
        "enforcement": "active",
        "bypass_actors": [actor],
        "rules": []
    })
}

#[test]
fn branch_rules_entry_is_resolved_to_the_documented_ruleset_detail_route() {
    let effective_rule = serde_json::json!({
        "type": "required_status_checks",
        "ruleset_source_type": "Organization",
        "ruleset_source": "acme",
        "ruleset_id": 73,
        "parameters": {"strict_required_status_checks_policy": true}
    });
    assert_eq!(
        ruleset_detail_route("acme/homebrew-tap", 73, "Organization", "acme")
            .expect("organization ruleset route"),
        "orgs/acme/rulesets/73"
    );
    assert_eq!(
        ruleset_detail_route(
            "acme/homebrew-tap",
            effective_rule["ruleset_id"].as_u64().expect("ruleset id"),
            effective_rule["ruleset_source_type"]
                .as_str()
                .expect("source type"),
            effective_rule["ruleset_source"].as_str().expect("source"),
        )
        .expect("effective API rule route"),
        "orgs/acme/rulesets/73"
    );
    assert_eq!(
        ruleset_detail_route("acme/homebrew-tap", 42, "Repository", "acme/homebrew-tap")
            .expect("repository ruleset route"),
        "repos/acme/homebrew-tap/rulesets/42?includes_parents=true"
    );
    assert!(ruleset_detail_route("acme/homebrew-tap", 42, "Organization", "acme/other").is_err());
}

#[test]
fn promotion_requires_exact_dco_and_completion_check_app_bindings() {
    let valid = vec![("DCO".to_owned(), 19), ("Review complete".to_owned(), 23)];
    assert!(validate_required_check_bindings(&valid, "Review complete", 23).is_ok());
    assert!(validate_required_check_bindings(&valid, "Review complete", 24).is_err());
    assert!(
        validate_required_check_bindings(
            &[
                ("DCO validation".to_owned(), 19),
                ("Review complete".to_owned(), 23)
            ],
            "Review complete",
            23
        )
        .is_err()
    );
}
