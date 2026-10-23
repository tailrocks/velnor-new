use serde_json::json;

use super::{
    validate_effective_main_rules, validate_environment_branch_policy,
    validate_environment_settings,
};

fn valid_rules() -> serde_json::Value {
    json!([
        {"type":"pull_request","parameters":{"required_approving_review_count":1,"required_review_thread_resolution":true}},
        {"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI"}]}},
        {"type":"non_fast_forward"},
        {"type":"deletion"}
    ])
}

#[test]
fn effective_rules_accept_composed_main_protection() {
    assert!(validate_effective_main_rules(&valid_rules()).is_ok());
}

#[test]
fn effective_rules_reject_missing_pull_request_checks_force_push_or_deletion_guard() {
    let baseline = valid_rules();
    for omitted in [
        "pull_request",
        "required_status_checks",
        "non_fast_forward",
        "deletion",
    ] {
        let filtered = baseline
            .as_array()
            .expect("array")
            .iter()
            .filter(|rule| rule.get("type").and_then(serde_json::Value::as_str) != Some(omitted))
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            validate_effective_main_rules(&json!(filtered)).is_err(),
            "missing {omitted} must reject"
        );
    }
    let no_context =
        json!([{"type":"required_status_checks","parameters":{"required_status_checks":[{}]}}]);
    assert!(validate_effective_main_rules(&no_context).is_err());
    let mut zero_approvals = valid_rules();
    zero_approvals[0]["parameters"]["required_approving_review_count"] = json!(0);
    assert!(validate_effective_main_rules(&zero_approvals).is_ok());
    let mut missing_approval_count = valid_rules();
    missing_approval_count[0]["parameters"] = json!({});
    assert!(validate_effective_main_rules(&missing_approval_count).is_err());
    let mut unresolved_threads = valid_rules();
    unresolved_threads[0]["parameters"]["required_review_thread_resolution"] = json!(false);
    assert!(validate_effective_main_rules(&unresolved_threads).is_err());
    assert!(validate_effective_main_rules(&json!({})).is_err());
}

#[test]
fn classic_branch_protection_is_validated() {
    let classic = json!({
        "required_pull_request_reviews":{"required_approving_review_count":0},
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":{"contexts":["CI"]},
        "allow_force_pushes":{"enabled":false},
        "allow_deletions":{"enabled":false}
    });
    assert!(super::validate_main_protection_sources(&json!([]), Some(&classic)).is_ok());
}

#[test]
fn classic_and_ruleset_controls_combine_across_sources() {
    let partial_ruleset = json!([
        {"type":"pull_request","parameters":{"required_approving_review_count":1,"required_review_thread_resolution":false}},
        {"type":"non_fast_forward"},
        {"type":"deletion"}
    ]);
    let partial_classic = json!({
        "required_pull_request_reviews":null,
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":{"contexts":["CI"]},
        "allow_force_pushes":null,
        "allow_deletions":null
    });
    assert!(
        super::validate_main_protection_sources(&partial_ruleset, Some(&partial_classic)).is_ok()
    );
}

#[test]
fn classic_and_ruleset_can_supply_thread_resolution_from_either_source() {
    let split_thread_rule = json!([
        {"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI"}]}}
    ]);
    let classic_with_thread = json!({
        "required_pull_request_reviews":{"required_approving_review_count":1},
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":null,
        "allow_force_pushes":{"enabled":false},
        "allow_deletions":{"enabled":false}
    });
    let ruleset_with_pull_request = json!([
        {"type":"pull_request","parameters":{"required_approving_review_count":1,"required_review_thread_resolution":true}},
        {"type":"non_fast_forward"},
        {"type":"deletion"}
    ]);
    assert!(
        super::validate_main_protection_sources(
            &ruleset_with_pull_request,
            Some(&json!({
                "required_pull_request_reviews":null,
                "required_conversation_resolution":null,
                "required_status_checks":{"contexts":["CI"]},
                "allow_force_pushes":null,
                "allow_deletions":null
            }))
        )
        .is_ok()
    );
    let ruleset_missing_thread = json!([
        {"type":"pull_request","parameters":{"required_approving_review_count":1,"required_review_thread_resolution":false}},
        {"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI"}]}},
        {"type":"non_fast_forward"},
        {"type":"deletion"}
    ]);
    assert!(
        super::validate_main_protection_sources(
            &ruleset_missing_thread,
            Some(&json!({"required_conversation_resolution":{"enabled":true}}))
        )
        .is_ok()
    );
    assert!(
        super::validate_main_protection_sources(&split_thread_rule, Some(&classic_with_thread))
            .is_ok()
    );
}

#[test]
fn malformed_present_protection_fields_fail_even_when_other_source_is_complete() {
    let classic = json!({
        "required_pull_request_reviews":{"required_approving_review_count":0},
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":{"contexts":["CI"]},
        "allow_force_pushes":{"enabled":false},
        "allow_deletions":{"enabled":false}
    });
    let complete_rules = valid_rules();
    let malformed_classic = json!({"allow_deletions":{"enabled":"false"}});
    assert!(
        super::validate_main_protection_sources(&complete_rules, Some(&malformed_classic)).is_err()
    );
    let malformed_status = json!({"required_status_checks":{"contexts":[null]}});
    assert!(
        super::validate_main_protection_sources(&complete_rules, Some(&malformed_status)).is_err()
    );
    let malformed_rules = json!([
        {"type":"pull_request","parameters":{"required_approving_review_count":1}},
        {"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"CI"}]}},
        {"type":"non_fast_forward"},
        {"type":"deletion"}
    ]);
    assert!(super::validate_main_protection_sources(&malformed_rules, Some(&classic)).is_err());
    assert!(
        super::validate_main_protection_sources(
            &json!([]),
            Some(&json!({"required_pull_request_reviews":{}}))
        )
        .is_err()
    );
    assert!(super::validate_main_protection_sources(&json!([]), None).is_err());
    let allows_force_push = json!({
        "required_pull_request_reviews":{"required_approving_review_count":0},
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":{"contexts":["CI"]},
        "allow_force_pushes":{"enabled":true},
        "allow_deletions":{"enabled":false}
    });
    assert!(super::validate_main_protection_sources(&json!([]), Some(&allows_force_push)).is_err());
}

#[test]
fn stable_environment_preserves_human_reviewer_and_protected_branch_requirements() {
    let stable = json!({
        "name":"release",
        "can_admins_bypass":false,
        "protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"id":17,"login":"release-reviewer"}}],"prevent_self_review":true}],
        "deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false}
    });
    assert!(super::validate_stable_environment_settings(&stable, "main").is_ok());
    for wrong_name in ["consumer-binary-release", "preview-publish", "tap-update"] {
        let mut wrong_environment = stable.clone();
        wrong_environment["name"] = json!(wrong_name);
        assert!(super::validate_stable_environment_settings(&wrong_environment, "main").is_err());
    }

    assert!(super::validate_stable_environment_settings(&stable, "release/1.2").is_ok());
    let mut additional_rules = stable.clone();
    additional_rules["protection_rules"] = json!([
        {"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"id":17,"login":"release-reviewer"}}],"prevent_self_review":true},
        {"type":"wait_timer","wait_timer":15},
        {"type":"custom","app_id":42}
    ]);
    assert!(super::validate_stable_environment_settings(&additional_rules, "release/1.2").is_ok());

    let mut missing_reviewers = stable.clone();
    missing_reviewers["protection_rules"][0]["reviewers"] = json!([]);
    assert!(super::validate_stable_environment_settings(&missing_reviewers, "main").is_err());

    for invalid_entry in [
        json!(null),
        json!({}),
        json!({"type":"Bot","reviewer":{"id":17}}),
        json!({"type":"User","reviewer":{}}),
        json!({"type":"Team","reviewer":{"id":0}}),
    ] {
        let mut invalid = stable.clone();
        invalid["protection_rules"][0]["reviewers"] = json!([invalid_entry]);
        assert!(super::validate_stable_environment_settings(&invalid, "main").is_err());
    }

    let mut team_reviewer = stable.clone();
    team_reviewer["protection_rules"][0]["reviewers"] =
        json!([{"type":"Team","reviewer":{"id":29,"name":"release-team"}}]);
    assert!(super::validate_stable_environment_settings(&team_reviewer, "main").is_ok());

    let mut self_review = stable.clone();
    self_review["protection_rules"][0]["prevent_self_review"] = json!(false);
    assert!(super::validate_stable_environment_settings(&self_review, "main").is_err());

    let mut unprotected = stable.clone();
    unprotected["deployment_branch_policy"]["protected_branches"] = json!(false);
    assert!(super::validate_stable_environment_settings(&unprotected, "main").is_err());
    assert!(super::validate_stable_environment_settings(&stable, "bad..branch").is_err());
}

fn env_settings(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "protection_rules": [],
        "deployment_branch_policy": {
            "protected_branches": false,
            "custom_branch_policies": true
        }
    })
}

#[test]
fn publishing_environments_require_exact_name_no_approval_rule_and_custom_branch_mode() {
    assert!(
        validate_environment_settings(&env_settings("preview-publish"), "preview-publish").is_ok()
    );
    assert!(validate_environment_settings(&env_settings("tap-update"), "tap-update").is_ok());
    assert!(validate_environment_settings(&env_settings("other"), "preview-publish").is_err());

    let mut review = env_settings("preview-publish");
    review["protection_rules"] = json!([{"type":"required_reviewers"}]);
    assert!(validate_environment_settings(&review, "preview-publish").is_err());

    // GitHub lists the custom deployment branch policy itself as a `branch_policy` rule
    // (captured from jackin-project/jackin `preview-publish`, 2026-10-23).
    let mut branch_policy = env_settings("preview-publish");
    branch_policy["protection_rules"] =
        json!([{"id":69_193_833,"node_id":"GA_kwDOR2N26c4EH9Bp","type":"branch_policy"}]);
    assert!(validate_environment_settings(&branch_policy, "preview-publish").is_ok());
    for rules in [
        json!([{"type":"branch_policy"},{"type":"required_reviewers"}]),
        json!([{"type":"branch_policy"},{"type":"branch_policy"}]),
        json!([{"type":"wait_timer","wait_timer":5}]),
        json!([{"id":1}]),
        json!({}),
        json!(null),
    ] {
        let mut changed = env_settings("preview-publish");
        changed["protection_rules"] = rules;
        assert!(validate_environment_settings(&changed, "preview-publish").is_err());
    }

    for (protected, custom) in [(true, false), (false, false), (true, true)] {
        let mut invalid = env_settings("preview-publish");
        invalid["deployment_branch_policy"] = json!({
            "protected_branches": protected,
            "custom_branch_policies": custom
        });
        assert!(validate_environment_settings(&invalid, "preview-publish").is_err());
    }
    assert!(
        validate_environment_settings(&json!({"name":"preview-publish"}), "preview-publish")
            .is_err()
    );
}

#[test]
fn environment_branch_policy_must_be_the_only_exact_main_pattern() {
    let only_main = json!({"total_count":1,"branch_policies":[{"id":7,"name":"main"}]});
    assert!(validate_environment_branch_policy(&only_main, "main").is_ok());
    for bad in [
        json!({"total_count":0,"branch_policies":[]}),
        json!({"total_count":2,"branch_policies":[{"name":"main"},{"name":"release/*"}]}),
        json!({"total_count":1,"branch_policies":[{"name":"main*"}]}),
        json!({"total_count":1,"branch_policies":[{"name":"refs/heads/main"}]}),
        json!({"total_count":1,"branch_policies":[]}),
    ] {
        assert!(validate_environment_branch_policy(&bad, "main").is_err());
    }
    assert!(validate_environment_branch_policy(&only_main, "release/1.2").is_err());
}

#[test]
fn permanent_tag_release_environment_requires_human_review_without_self_approval() {
    let settings = json!({"name":"release", "can_admins_bypass":false, "deployment_branch_policy":{
        "protected_branches":true,"custom_branch_policies":false},
        "protection_rules":[{"type":"required_reviewers","prevent_self_review":true,
            "reviewers":[{"type":"User","reviewer":{"id":17}}]}]});
    assert!(
        super::validation::validate_human_environment_settings(&settings, "main", "release")
            .is_ok()
    );
    for bypass in [None, Some(json!(null)), Some(json!(true))] {
        let mut changed = settings.clone();
        changed
            .as_object_mut()
            .expect("environment")
            .remove("can_admins_bypass");
        if let Some(value) = bypass {
            changed["can_admins_bypass"] = value;
        }
        assert!(super::validate_stable_environment_settings(&changed, "main").is_err());
    }
    for mutate in [
        |value: &mut serde_json::Value| value["name"] = json!("other"),
        |value: &mut serde_json::Value| {
            value["protection_rules"][0]["prevent_self_review"] = json!(false);
        },
        |value: &mut serde_json::Value| value["protection_rules"][0]["reviewers"] = json!([]),
        |value: &mut serde_json::Value| {
            value["protection_rules"][0]["reviewers"][0]["type"] = json!("Bot");
        },
    ] {
        let mut changed = settings.clone();
        mutate(&mut changed);
        assert!(
            super::validation::validate_human_environment_settings(&changed, "main", "release")
                .is_err()
        );
    }
}

#[test]
fn status_reader_routes_are_closed_to_same_repository_metadata_and_settings() {
    for suffix in [
        "",
        "/branches/main",
        "/rules/branches/main",
        "/branches/main/protection",
        "/environments/release",
        "/environments/preview-publish",
        "/environments/tap-update",
        "/environments/tap-update/deployment-branch-policies?per_page=100&page=1",
    ] {
        assert!(
            super::validate_status_route("example/app", &format!("repos/example/app{suffix}"))
                .is_ok()
        );
    }
    for route in [
        "repos/foreign/app",
        "repos/example/app/contents/Cargo.toml",
        "repos/example/app/actions/runs/22",
        "repos/example/app/actions/artifacts/22",
        "repos/example/app/releases",
        "repos/example/app/environments/other",
        "repos/example/app/branches/other",
        "repos/example/app/environments/release/deployment-branch-policies?per_page=100&page=2",
        "https://api.github.com/repos/example/app",
    ] {
        assert!(
            super::validate_status_route("example/app", route).is_err(),
            "{route}"
        );
    }
}

#[test]
fn current_source_sha_and_protection_come_from_one_branch_snapshot() {
    let valid = json!({"name":"main","protected":true,"commit":{"sha":"a".repeat(40)}});
    assert_eq!(
        super::protected_branch_sha(&valid).expect("protected main"),
        "a".repeat(40)
    );
    for (pointer, replacement) in [
        ("/name", json!("other")),
        ("/protected", json!(false)),
        ("/commit/sha", json!("bad")),
        ("/commit", serde_json::Value::Null),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).expect("field") = replacement;
        assert!(super::protected_branch_sha(&changed).is_err(), "{pointer}");
    }
}
