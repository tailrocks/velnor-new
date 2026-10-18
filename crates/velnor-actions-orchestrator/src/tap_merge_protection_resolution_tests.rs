use super::super::super::github::{ApiResponse, TapMergeGitHub};
use super::super::*;
use super::super::{ProtectionAuthority, resolve_protection};
use std::collections::BTreeMap;

struct ProtectionFixture {
    classic_review: Option<Value>,
    admin_policy: AdminPolicy,
    classic_checks: CheckMode,
    ruleset_checks: CheckMode,
    pull_request: Option<Value>,
    classic_extra_checks: Vec<Value>,
    ruleset_extra_checks: Vec<Value>,
    classic_endpoint: ClassicEndpoint,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AdminPolicy {
    Enforced,
    Exempt,
}

#[derive(Clone, Copy)]
enum CheckMode {
    Strict,
    Loose,
    EmptyStrict,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ClassicEndpoint {
    Present,
    Absent,
}

impl ProtectionFixture {
    fn new(
        admin_policy: AdminPolicy,
        classic_checks: CheckMode,
        ruleset_checks: CheckMode,
        pull_request: Option<Value>,
    ) -> Self {
        Self {
            classic_review: None,
            admin_policy,
            classic_checks,
            ruleset_checks,
            pull_request,
            classic_extra_checks: Vec::new(),
            ruleset_extra_checks: Vec::new(),
            classic_endpoint: ClassicEndpoint::Present,
        }
    }
}

#[test]
fn resolver_unions_classic_checks_with_ruleset_checks_without_classic_review_settings() {
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Loose,
        None,
    ));
    let authority = protection_authority();
    let protection = resolve_protection(&api, "owner/tap", "main", &authority)
        .expect("composed protection sources");
    assert!(protection.strict);
    assert!(!protection.writer_bypass);
    assert_eq!(
        protection.checks,
        vec![("DCO".to_owned(), 19), ("Review completion".to_owned(), 23)]
    );
}

#[test]
fn nonstrict_ruleset_alone_does_not_claim_server_strictness() {
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Loose,
        CheckMode::Loose,
        None,
    ));
    let authority = protection_authority();
    let protection = resolve_protection(&api, "owner/tap", "main", &authority)
        .expect("loose source remains observable");
    assert!(!protection.strict);
}

#[test]
fn empty_strict_ruleset_does_not_establish_up_to_date_protection() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Loose,
        CheckMode::EmptyStrict,
        None,
    );
    fixture
        .classic_extra_checks
        .push(serde_json::json!({"context":"Review completion", "app_id":23}));
    let api = scripted_protection_api(&fixture);
    let authority = protection_authority();
    let protection = resolve_protection(&api, "owner/tap", "main", &authority)
        .expect("loose classic checks remain observable");
    assert!(!protection.strict);
    assert!(
        protection
            .checks
            .contains(&("Review completion".to_owned(), 23))
    );
}

#[test]
fn empty_classic_strict_rule_does_not_establish_up_to_date_protection() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::EmptyStrict,
        CheckMode::Loose,
        None,
    );
    fixture
        .ruleset_extra_checks
        .push(serde_json::json!({"context":"Review completion", "integration_id":23}));
    let api = scripted_protection_api(&fixture);
    let authority = protection_authority();
    let protection = resolve_protection(&api, "owner/tap", "main", &authority)
        .expect("loose ruleset checks remain observable");
    assert!(!protection.strict);
    assert!(
        protection
            .checks
            .contains(&("Review completion".to_owned(), 23))
    );
}

#[test]
fn resolver_does_not_hide_writer_bypass_when_review_settings_are_present() {
    let review = serde_json::json!({
        "bypass_pull_request_allowances": {
            "users": [], "teams": [], "apps": [{"id": 17}]
        }
    });
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Strict,
        None,
    );
    fixture.classic_review = Some(review);
    let api = scripted_protection_api(&fixture);
    let authority = protection_authority();
    let protection = resolve_protection(&api, "owner/tap", "main", &authority)
        .expect("protection facts remain observable");
    assert!(protection.strict);
    assert!(protection.writer_bypass);
}

#[test]
fn ruleset_protection_covers_classic_admin_exemption_only_with_pr_and_exact_strict_checks() {
    let pull_request = serde_json::json!({
        "required_approving_review_count": 0,
        "required_review_thread_resolution": true
    });
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Loose,
        CheckMode::Strict,
        Some(pull_request),
    ));
    let authority = protection_authority();
    let protection = resolve_protection(&api, "owner/tap", "main", &authority)
        .expect("complete strict ruleset alternative");
    assert!(protection.strict);
    assert!(!protection.writer_bypass);
}

#[test]
fn admin_fallback_requires_rulesets_to_cover_every_classic_required_check() {
    let pull_request = serde_json::json!({
        "required_approving_review_count": 0,
        "required_review_thread_resolution": true
    });
    let mut missing_check = ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Strict,
        CheckMode::Strict,
        Some(pull_request.clone()),
    );
    missing_check
        .classic_extra_checks
        .push(serde_json::json!({"context":"CI", "app_id":29}));
    let missing_api = scripted_protection_api(&missing_check);
    let authority = protection_authority();
    assert!(resolve_protection(&missing_api, "owner/tap", "main", &authority).is_err());

    let mut covered = ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Strict,
        CheckMode::Strict,
        Some(pull_request),
    );
    covered
        .classic_extra_checks
        .push(serde_json::json!({"context":"CI", "app_id":29}));
    covered
        .ruleset_extra_checks
        .push(serde_json::json!({"context":"CI", "integration_id":29}));
    let covered_api = scripted_protection_api(&covered);
    let protection = resolve_protection(&covered_api, "owner/tap", "main", &authority)
        .expect("all classic checks also protected by the strict ruleset");
    assert!(protection.strict);
}

#[test]
fn absent_classic_protection_does_not_require_admin_replacement_rule() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Loose,
        CheckMode::Strict,
        None,
    );
    fixture.classic_endpoint = ClassicEndpoint::Absent;
    let api = scripted_protection_api(&fixture);
    let authority = protection_authority();
    assert!(
        resolve_protection(&api, "owner/tap", "main", &authority)
            .expect("effective strict ruleset without classic settings")
            .strict
    );
}

#[test]
fn classic_admin_exemption_requires_authenticated_ruleset_replacement() {
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Strict,
        CheckMode::Strict,
        None,
    ));
    let authority = protection_authority();
    assert!(resolve_protection(&api, "owner/tap", "main", &authority).is_err());
}

#[test]
fn malformed_ruleset_pr_replacement_is_rejected() {
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Strict,
        CheckMode::Strict,
        Some(serde_json::json!({})),
    ));
    let authority = protection_authority();
    assert!(resolve_protection(&api, "owner/tap", "main", &authority).is_err());
}

fn protection_authority() -> ProtectionAuthority<'static> {
    ProtectionAuthority {
        writer_app_id: 17,
        writer_bot_user_id: 29,
        writer_bot_login: "tap-bot[bot]",
        completion_check_name: "Review completion",
        completion_app_id: 23,
    }
}

fn scripted_protection_api(fixture: &ProtectionFixture) -> TapMergeGitHub {
    let classic_present = fixture.classic_endpoint == ClassicEndpoint::Present;
    let classic = classic_response(fixture);
    let mut responses = BTreeMap::from([
        (
            "repos/owner/tap/branches/main/protection".to_owned(),
            classic,
        ),
        (
            "repos/owner/tap/rules/branches/main?per_page=100&page=1".to_owned(),
            serde_json::json!([{
                "type":"required_status_checks",
                "ruleset_id":41,
                "ruleset_source_type":"Repository",
                "ruleset_source":"owner/tap"
            }]),
        ),
        (
            "repos/owner/tap/rulesets/41?includes_parents=true".to_owned(),
            ruleset_response(fixture),
        ),
    ]);
    TapMergeGitHub::with_test_transport(move |args| {
        let route = args
            .first()
            .and_then(|arg| arg.to_str())
            .ok_or_else(|| crate::internal::internal("test_route_missing"))?;
        if route == "repos/owner/tap/branches/main/protection" && !classic_present {
            return Ok(ApiResponse {
                status: 404,
                body: b"{}".to_vec(),
            });
        }
        let value = responses
            .remove(route)
            .ok_or_else(|| crate::internal::internal("unexpected_protection_route"))?;
        Ok(ApiResponse {
            status: 200,
            body: serde_json::to_vec(&value)
                .map_err(|_| crate::internal::internal("test_response_encode"))?,
        })
    })
}

fn classic_response(fixture: &ProtectionFixture) -> Value {
    let mut checks = if matches!(fixture.classic_checks, CheckMode::EmptyStrict) {
        Vec::new()
    } else {
        vec![serde_json::json!({"context":"DCO", "app_id":19})]
    };
    checks.extend(fixture.classic_extra_checks.iter().cloned());
    let mut response = serde_json::json!({
        "enforce_admins": {"enabled": fixture.admin_policy == AdminPolicy::Enforced},
        "required_status_checks": {
            "strict":matches!(fixture.classic_checks, CheckMode::Strict | CheckMode::EmptyStrict),
            "checks":checks
        }
    });
    response["required_pull_request_reviews"] =
        fixture.classic_review.clone().unwrap_or(Value::Null);
    response
}

fn ruleset_response(fixture: &ProtectionFixture) -> Value {
    let mut checks = if matches!(fixture.ruleset_checks, CheckMode::EmptyStrict) {
        Vec::new()
    } else {
        vec![
            serde_json::json!({"context":"DCO", "integration_id":19}),
            serde_json::json!({"context":"Review completion", "integration_id":23}),
        ]
    };
    checks.extend(fixture.ruleset_extra_checks.iter().cloned());
    let mut rules = vec![serde_json::json!({
        "type":"required_status_checks",
        "parameters":{
            "strict_required_status_checks_policy":matches!(fixture.ruleset_checks, CheckMode::Strict | CheckMode::EmptyStrict),
            "required_status_checks":checks
        }
    })];
    if let Some(parameters) = &fixture.pull_request {
        rules.push(serde_json::json!({"type":"pull_request","parameters":parameters}));
    }
    serde_json::json!({
        "id":41,
        "source_type":"Repository",
        "source":"owner/tap",
        "target":"branch",
        "enforcement":"active",
        "bypass_actors":[],
        "rules":rules
    })
}
