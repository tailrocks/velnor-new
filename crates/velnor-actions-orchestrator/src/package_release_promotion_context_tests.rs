use super::*;
use velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1;
fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![],
        companions: vec![],
    }
}
fn stable_plan() -> PackageReleasePlan {
    PackageReleasePlan {
        channel: PackageReleaseChannel::Stable,
        ..plan()
    }
}
fn invocation(event: &str, path: &str) -> Invocation {
    Invocation {
        repository: "example/app".to_owned(),
        source_ref: "refs/heads/main".to_owned(),
        source_sha: "a".repeat(40),
        workflow_sha: "a".repeat(40),
        event: event.to_owned(),
        workflow_ref: format!("example/app/{path}@refs/heads/main"),
    }
}
#[test]
fn continuation_never_grants_publication_recovery_authority() {
    for event in ["schedule", "workflow_dispatch"] {
        let caller = invocation(event, CONTINUATION_PATH);
        assert!(caller.validate(&plan(), Purpose::TapPromotion).is_ok());
        assert!(
            caller
                .validate(&plan(), Purpose::PublicationRecovery)
                .is_err()
        );
    }
    let caller = invocation("workflow_dispatch", plan().channel.workflow_path());
    assert!(
        caller
            .validate(&plan(), Purpose::PublicationRecovery)
            .is_ok()
    );
}
#[test]
fn stable_continuation_uses_only_its_channel_workflow_identity() {
    let stable = stable_plan();
    let stable_path = stable.channel.workflow_path();
    let caller = invocation("schedule", ".github/workflows/stable-tap-reconcile.yml");
    assert!(caller.validate(&stable, Purpose::TapPromotion).is_ok());
    assert!(
        caller
            .validate(&stable, Purpose::PublicationRecovery)
            .is_err()
    );

    let wrong_workflow = invocation("schedule", CONTINUATION_PATH);
    assert!(
        wrong_workflow
            .validate(&stable, Purpose::TapPromotion)
            .is_err()
    );
    let manual = invocation("workflow_dispatch", stable_path);
    assert!(
        manual
            .validate(&stable, Purpose::PublicationRecovery)
            .is_ok()
    );
}
#[test]
fn continuation_rejects_foreign_branch_workflow_repository_and_source() {
    for field in 0..6 {
        let mut caller = invocation("schedule", CONTINUATION_PATH);
        match field {
            0 => caller.repository = "foreign/app".to_owned(),
            1 => caller.source_ref = "refs/heads/other".to_owned(),
            2 => caller.workflow_sha = "b".repeat(40),
            3 => caller.source_sha = "bad".to_owned(),
            4 => {
                caller.workflow_ref =
                    "example/app/.github/workflows/other.yml@refs/heads/main".to_owned();
            }
            _ => caller.event = "push".to_owned(),
        }
        assert!(caller.validate(&plan(), Purpose::TapPromotion).is_err());
    }
}
