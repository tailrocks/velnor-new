//! Classic branch-protection absence and ruleset-only main protection (#354).

use serde_json::json;

#[test]
fn only_exact_absent_classic_protection_responses_are_treated_as_absent() {
    let cli_stderr = b"gh: Not Found (HTTP 404)";
    let cli_body = br#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest/branches/branch-protection#get-branch-protection","status":"404"}"#;
    assert!(super::classic_protection_absent(cli_stderr, cli_body));
    assert!(!super::classic_protection_absent(
        cli_stderr,
        br#"{"message":"Not Found","status":"403"}"#
    ));
    assert!(!super::classic_protection_absent(
        b"gh: Forbidden (HTTP 403)",
        cli_body
    ));
    assert!(!super::classic_protection_absent(cli_stderr, b""));
}

/// Captured 2026-10-23 from `gh api repos/jackin-project/jackin/branches/main/protection`
/// (main protected only by rulesets; caller has Administration read).
const RULESET_ONLY_STDERR: &[u8] = b"gh: Branch not protected (HTTP 404)";
const RULESET_ONLY_BODY: &[u8] = br#"{"message":"Branch not protected","documentation_url":"https://docs.github.com/rest/branches/branch-protection#get-branch-protection","status":"404"}"#;

#[test]
fn ruleset_only_branch_not_protected_response_is_absent_classic_protection() {
    assert!(super::classic_protection_absent(
        RULESET_ONLY_STDERR,
        RULESET_ONLY_BODY
    ));
}

#[test]
fn other_classic_protection_failures_still_fail_closed() {
    for (stderr, body) in [
        (
            &b"gh: Resource not accessible by personal access token (HTTP 403)"[..],
            &br#"{"message":"Resource not accessible by personal access token","status":"403"}"#[..],
        ),
        (
            &b"gh: Bad credentials (HTTP 401)"[..],
            &br#"{"message":"Bad credentials","status":"401"}"#[..],
        ),
        (
            &b"gh: Branch not protected (HTTP 403)"[..],
            RULESET_ONLY_BODY,
        ),
        (
            RULESET_ONLY_STDERR,
            &br#"{"message":"Branch not protected","status":"403"}"#[..],
        ),
        (
            RULESET_ONLY_STDERR,
            &br#"{"message":"Branch not protected.","status":"404"}"#[..],
        ),
        (
            RULESET_ONLY_STDERR,
            &br#"{"message":"Repository not found","status":"404"}"#[..],
        ),
        (RULESET_ONLY_STDERR, &b"Branch not protected"[..]),
    ] {
        assert!(!super::classic_protection_absent(stderr, body));
    }
}

#[test]
fn ruleset_only_main_protection_is_complete_without_classic_source() {
    // Captured effective rules for jackin-project/jackin main (2026-10-23).
    let rules = json!([
        {"type":"deletion","parameters":null},
        {"type":"pull_request","parameters":{"allowed_merge_methods":["squash"],"dismiss_stale_reviews_on_push":true,"require_code_owner_review":false,"require_last_push_approval":false,"required_approving_review_count":0,"required_review_thread_resolution":true,"required_reviewers":[]}},
        {"type":"required_status_checks","parameters":{"do_not_enforce_on_create":false,"required_status_checks":[{"context":"DCO","integration_id":974_774},{"context":"Required","integration_id":15368}],"strict_required_status_checks_policy":true}},
        {"type":"non_fast_forward","parameters":null},
        {"type":"required_linear_history","parameters":null}
    ]);
    assert!(super::validate_main_protection_sources(&rules, None).is_ok());
    let without_deletion = json!(rules.as_array().expect("rules")[1..].to_vec());
    assert!(super::validate_main_protection_sources(&without_deletion, None).is_err());
}
