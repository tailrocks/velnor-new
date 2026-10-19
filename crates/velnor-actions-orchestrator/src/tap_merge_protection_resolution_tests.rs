use super::super::{BranchControl, resolve_protection};
use super::support::{
    AdminPolicy, CheckMode, ClassicEndpoint, ProtectionFixture, protection_authority,
    scripted_protection_api,
};

#[test]
fn resolver_unions_classic_controls_and_required_checks_from_both_sources() {
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Loose,
        None,
    ));
    let authority = protection_authority();
    let protection = resolve_protection(&api, &api, "owner/tap", "main", &authority)
        .expect("composed protection sources");
    assert!(protection.strict);
    assert!(!protection.writer_bypass);
    assert_eq!(
        protection.checks,
        vec![("DCO".to_owned(), 19), ("Review completion".to_owned(), 23)]
    );
}

#[test]
fn nonstrict_ruleset_cannot_authorize_a_tap_merge() {
    let api = scripted_protection_api(&ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Loose,
        CheckMode::Loose,
        None,
    ));
    let authority = protection_authority();
    assert!(resolve_protection(&api, &api, "owner/tap", "main", &authority).is_err());
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
    assert!(resolve_protection(&api, &api, "owner/tap", "main", &authority).is_err());
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
    assert!(resolve_protection(&api, &api, "owner/tap", "main", &authority).is_err());
}

#[test]
fn resolver_does_not_hide_writer_bypass_when_review_settings_are_present() {
    let review = serde_json::json!({
        "required_approving_review_count": 0,
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
    let protection = resolve_protection(&api, &api, "owner/tap", "main", &authority)
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
    let protection = resolve_protection(&api, &api, "owner/tap", "main", &authority)
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
    assert!(
        resolve_protection(&missing_api, &missing_api, "owner/tap", "main", &authority).is_err()
    );

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
    let protection =
        resolve_protection(&covered_api, &covered_api, "owner/tap", "main", &authority)
            .expect("all classic checks also protected by the strict ruleset");
    assert!(protection.strict);
}

#[test]
fn absent_classic_protection_does_not_require_admin_replacement_rule() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Exempt,
        CheckMode::Loose,
        CheckMode::Strict,
        Some(serde_json::json!({
            "required_approving_review_count": 0,
            "required_review_thread_resolution": true
        })),
    );
    fixture.classic_endpoint = ClassicEndpoint::Absent;
    let api = scripted_protection_api(&fixture);
    let authority = protection_authority();
    assert!(
        resolve_protection(&api, &api, "owner/tap", "main", &authority)
            .expect("effective strict ruleset without classic settings")
            .strict
    );
}

#[test]
fn checks_only_protection_cannot_authorize_direct_push_as_a_pr_promotion() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Strict,
        None,
    );
    fixture.set_classic_control(BranchControl::PullRequest, false);
    fixture.set_classic_control(BranchControl::ThreadResolution, false);
    fixture.set_classic_control(BranchControl::NoForcePush, false);
    fixture.set_classic_control(BranchControl::NoDeletion, false);
    assert!(resolve_fixture(&fixture).is_err());

    fixture.admin_policy = AdminPolicy::Exempt;
    assert!(resolve_fixture(&fixture).is_err());
}

#[test]
fn required_pr_without_resolved_threads_is_not_merge_authority() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Strict,
        Some(serde_json::json!({
            "required_approving_review_count": 0,
            "required_review_thread_resolution": false
        })),
    );
    fixture.set_classic_control(BranchControl::PullRequest, false);
    fixture.set_classic_control(BranchControl::ThreadResolution, false);
    fixture.set_classic_control(BranchControl::NoForcePush, false);
    fixture.set_classic_control(BranchControl::NoDeletion, false);
    assert!(resolve_fixture(&fixture).is_err());
}

#[test]
fn classic_pr_and_ruleset_thread_controls_compose_independently() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Strict,
        Some(serde_json::json!({
            "required_approving_review_count": 0,
            "required_review_thread_resolution": false
        })),
    );
    fixture.set_classic_control(BranchControl::ThreadResolution, true);
    fixture.set_classic_control(BranchControl::NoForcePush, false);
    fixture.set_classic_control(BranchControl::NoDeletion, false);
    assert!(resolve_fixture(&fixture).is_ok());

    fixture.set_classic_control(BranchControl::PullRequest, false);
    fixture.set_classic_control(BranchControl::ThreadResolution, false);
    fixture.pull_request = Some(serde_json::json!({
        "required_approving_review_count": 0,
        "required_review_thread_resolution": true
    }));
    assert!(resolve_fixture(&fixture).is_ok());
}

#[test]
fn monotonic_tap_history_requires_force_push_and_deletion_blocks() {
    let mut fixture = ProtectionFixture::new(
        AdminPolicy::Enforced,
        CheckMode::Strict,
        CheckMode::Strict,
        Some(serde_json::json!({
            "required_approving_review_count": 0,
            "required_review_thread_resolution": true
        })),
    );
    fixture.set_classic_control(BranchControl::NoForcePush, false);
    fixture.set_ruleset_control(BranchControl::NoForcePush, false);
    assert!(resolve_fixture(&fixture).is_err());
    fixture.set_classic_control(BranchControl::NoForcePush, true);
    fixture.set_classic_control(BranchControl::NoDeletion, false);
    fixture.set_ruleset_control(BranchControl::NoDeletion, false);
    assert!(resolve_fixture(&fixture).is_err());
}

fn resolve_fixture(
    fixture: &ProtectionFixture,
) -> Result<super::super::ProtectionFacts, crate::OrchestratorError> {
    let api = scripted_protection_api(fixture);
    let authority = protection_authority();
    resolve_protection(&api, &api, "owner/tap", "main", &authority)
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
    assert!(resolve_protection(&api, &api, "owner/tap", "main", &authority).is_err());
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
    assert!(resolve_protection(&api, &api, "owner/tap", "main", &authority).is_err());
}

#[test]
fn classic_protection_uses_separate_observer_while_rulesets_use_the_writer() {
    use crate::tap_merge::github::{ApiResponse, TapMergeGitHub};
    for observer_status in [404, 403] {
        let fixture = ProtectionFixture::new(
            AdminPolicy::Exempt,
            CheckMode::Loose,
            CheckMode::Strict,
            Some(
                serde_json::json!({"required_approving_review_count":0,"required_review_thread_resolution":true}),
            ),
        );
        let delegate = scripted_protection_api(&fixture);
        let writer = TapMergeGitHub::with_test_transport(move |args| {
            let route = args.first().and_then(|arg| arg.to_str()).expect("route");
            assert!(
                !route.ends_with("/protection"),
                "writer must never read classic protection"
            );
            let value: serde_json::Value = delegate.get(route)?;
            Ok(ApiResponse {
                status: 200,
                body: serde_json::to_vec(&value).expect("JSON"),
            })
        });
        let observer = TapMergeGitHub::with_test_transport(move |args| {
            assert_eq!(args.len(), 1);
            assert_eq!(args[0], "repos/owner/tap/branches/main/protection");
            Ok(ApiResponse {
                status: observer_status,
                body: b"{}".to_vec(),
            })
        });
        let result = resolve_protection(
            &writer,
            &observer,
            "owner/tap",
            "main",
            &protection_authority(),
        );
        if observer_status == 404 {
            assert!(
                result
                    .expect("observer absence + writer strict rules")
                    .strict
            );
        } else {
            assert!(result.is_err());
        }
    }
}
