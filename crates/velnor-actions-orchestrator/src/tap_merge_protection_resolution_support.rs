use super::super::super::github::{ApiResponse, TapMergeGitHub};
use super::super::ProtectionAuthority;
use super::super::*;
use std::collections::BTreeMap;

pub(super) struct ProtectionFixture {
    pub(super) classic_review: Option<Value>,
    classic_controls: BranchControls,
    ruleset_controls: BranchControls,
    pub(super) admin_policy: AdminPolicy,
    pub(super) classic_checks: CheckMode,
    pub(super) ruleset_checks: CheckMode,
    pub(super) pull_request: Option<Value>,
    pub(super) classic_extra_checks: Vec<Value>,
    pub(super) ruleset_extra_checks: Vec<Value>,
    pub(super) classic_endpoint: ClassicEndpoint,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum AdminPolicy {
    Enforced,
    Exempt,
}

#[derive(Clone, Copy)]
pub(super) enum CheckMode {
    Strict,
    Loose,
    EmptyStrict,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ClassicEndpoint {
    Present,
    Absent,
}

impl ProtectionFixture {
    pub(super) fn new(
        admin_policy: AdminPolicy,
        classic_checks: CheckMode,
        ruleset_checks: CheckMode,
        pull_request: Option<Value>,
    ) -> Self {
        Self {
            classic_review: None,
            classic_controls: controls([
                BranchControl::PullRequest,
                BranchControl::ThreadResolution,
                BranchControl::NoForcePush,
                BranchControl::NoDeletion,
            ]),
            ruleset_controls: controls([BranchControl::NoForcePush, BranchControl::NoDeletion]),
            admin_policy,
            classic_checks,
            ruleset_checks,
            pull_request,
            classic_extra_checks: Vec::new(),
            ruleset_extra_checks: Vec::new(),
            classic_endpoint: ClassicEndpoint::Present,
        }
    }

    pub(super) fn set_classic_control(&mut self, control: BranchControl, enabled: bool) {
        self.classic_controls.set(control, enabled);
    }

    pub(super) fn set_ruleset_control(&mut self, control: BranchControl, enabled: bool) {
        self.ruleset_controls.set(control, enabled);
    }

    fn classic_has(&self, control: BranchControl) -> bool {
        self.classic_controls.contains(control)
    }

    fn ruleset_has(&self, control: BranchControl) -> bool {
        self.ruleset_controls.contains(control)
    }
}

fn controls(values: impl IntoIterator<Item = BranchControl>) -> BranchControls {
    let mut controls = BranchControls::default();
    for control in values {
        controls.insert(control);
    }
    controls
}
pub(super) fn protection_authority() -> ProtectionAuthority<'static> {
    ProtectionAuthority {
        writer_app_id: 17,
        writer_bot_user_id: 29,
        writer_bot_login: "tap-bot[bot]",
        completion_check_name: "Review completion",
        completion_app_id: 23,
    }
}

pub(super) fn scripted_protection_api(fixture: &ProtectionFixture) -> TapMergeGitHub {
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
        "required_conversation_resolution":{"enabled":fixture.classic_has(BranchControl::ThreadResolution)},
        "allow_force_pushes":{"enabled":!fixture.classic_has(BranchControl::NoForcePush)},
        "allow_deletions":{"enabled":!fixture.classic_has(BranchControl::NoDeletion)},
        "required_status_checks": {
            "strict":matches!(fixture.classic_checks, CheckMode::Strict | CheckMode::EmptyStrict),
            "checks":checks
        }
    });
    response["required_pull_request_reviews"] =
        fixture.classic_review.clone().unwrap_or_else(|| {
            if fixture.classic_has(BranchControl::PullRequest) {
                serde_json::json!({
                    "required_approving_review_count":0,
                    "bypass_pull_request_allowances":{"users":[],"teams":[],"apps":[]}
                })
            } else {
                Value::Null
            }
        });
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
    if fixture.ruleset_has(BranchControl::NoForcePush) {
        rules.push(serde_json::json!({"type":"non_fast_forward"}));
    }
    if fixture.ruleset_has(BranchControl::NoDeletion) {
        rules.push(serde_json::json!({"type":"deletion"}));
    }
    serde_json::json!({
        "id":41,
        "source_type":"Repository",
        "source":"owner/tap",
        "target":"branch",
        "enforcement":"active",
        "current_user_can_bypass":"never", "bypass_actors":[],
        "rules":rules
    })
}
