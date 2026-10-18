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
fn only_the_pinned_gh_cli_not_found_response_is_treated_as_absent_classic_protection() {
    let cli_stderr = b"gh: Not Found (HTTP 404)";
    let cli_body = br#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest/branches/branch-protection#get-branch-protection","status":"404"}"#;
    assert!(super::github_api_not_found(cli_stderr, cli_body));
    assert!(!super::github_api_not_found(
        cli_stderr,
        br#"{"message":"Not Found","status":"403"}"#
    ));
    assert!(!super::github_api_not_found(
        b"gh: Forbidden (HTTP 403)",
        cli_body
    ));
    assert!(!super::github_api_not_found(cli_stderr, b""));
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
    missing_approval_count[0]["parameters"] = json!({"required_review_thread_resolution":true});
    assert!(validate_effective_main_rules(&missing_approval_count).is_err());
    let mut unresolved_threads = valid_rules();
    unresolved_threads[0]["parameters"]["required_review_thread_resolution"] = json!(false);
    assert!(validate_effective_main_rules(&unresolved_threads).is_err());
    assert!(validate_effective_main_rules(&json!({})).is_err());
}

#[test]
fn classic_branch_protection_is_validated_and_can_complete_partial_rulesets() {
    let classic = json!({
        "required_pull_request_reviews":{"required_approving_review_count":0},
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":{"contexts":["CI"]},
        "allow_force_pushes":{"enabled":false},
        "allow_deletions":{"enabled":false}
    });
    assert!(super::validate_main_protection_sources(&json!([]), Some(&classic)).is_ok());

    let partial_ruleset = json!([
        {"type":"non_fast_forward"},
        {"type":"deletion"}
    ]);
    let partial_classic = json!({
        "required_pull_request_reviews":{"required_approving_review_count":0},
        "required_conversation_resolution":{"enabled":true},
        "required_status_checks":{"contexts":["CI"]}
    });
    assert!(
        super::validate_main_protection_sources(&partial_ruleset, Some(&partial_classic)).is_ok()
    );

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
        "name":"consumer-binary-release",
        "protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"login":"release-reviewer"}}],"prevent_self_review":true}],
        "deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false}
    });
    assert!(super::validate_stable_environment_settings(&stable, "main").is_ok());

    let mut missing_reviewers = stable.clone();
    missing_reviewers["protection_rules"][0]["reviewers"] = json!([]);
    assert!(super::validate_stable_environment_settings(&missing_reviewers, "main").is_err());

    let mut self_review = stable.clone();
    self_review["protection_rules"][0]["prevent_self_review"] = json!(false);
    assert!(super::validate_stable_environment_settings(&self_review, "main").is_err());

    let mut unprotected = stable;
    unprotected["deployment_branch_policy"]["protected_branches"] = json!(false);
    assert!(super::validate_stable_environment_settings(&unprotected, "main").is_err());
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
}
