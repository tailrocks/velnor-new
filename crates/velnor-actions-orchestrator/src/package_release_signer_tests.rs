use super::*;
use velnor_actions_contract::{
    PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};

fn plan(channel: PackageReleaseChannel) -> PackageReleasePlan {
    PackageReleasePlan {
        schema: velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["jackin".to_owned()],
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            format: if channel == PackageReleaseChannel::Stable {
                PackageReleasePayloadFormat::RawBinary
            } else {
                PackageReleasePayloadFormat::TarGz
            },
        }],
        companions: Vec::new(),
    }
}

fn identity(plan: &PackageReleasePlan) -> PackageReleaseSigningIdentity {
    PackageReleaseSigningIdentity {
        repository: plan.repository.clone(),
        repository_id: 42,
        source_sha: "a12e73afd75b944a872e8295d61a15f93f5789a2".to_owned(),
        source_ref: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        event: plan.channel.event(),
        workflow_path: plan.channel.workflow_path().to_owned(),
        workflow_ref: format!(
            "{}/{}@refs/heads/main",
            plan.repository,
            plan.channel.workflow_path()
        ),
        workflow_sha: "a12e73afd75b944a872e8295d61a15f93f5789a2".to_owned(),
        run_id: 10,
        run_attempt: 1,
        ci_run_id: 9,
        ci_run_attempt: 1,
        commit_count: (plan.channel == PackageReleaseChannel::Preview).then_some(7),
    }
}

#[test]
fn exact_channel_source_workflow_identity_is_required() {
    let preview = plan(PackageReleaseChannel::Preview);
    let mut facts = identity(&preview);
    assert!(validate_identity(&preview, &facts).is_ok());
    facts.workflow_path = ".github/workflows/untrusted.yml".to_owned();
    assert!(validate_identity(&preview, &facts).is_err());
    facts = identity(&preview);
    facts.event = PackageReleaseEvent::WorkflowDispatch;
    assert!(validate_identity(&preview, &facts).is_err());
    facts = identity(&preview);
    facts.workflow_sha = "b".repeat(40);
    assert!(validate_identity(&preview, &facts).is_err());
}

#[test]
fn signer_event_admission_rejects_old_push_and_wrong_channel_before_credentials() {
    let preview = plan(PackageReleaseChannel::Preview);
    assert_eq!(
        runtime::signing_event(&preview, "workflow_run").expect("preview"),
        PackageReleaseEvent::WorkflowRun
    );
    for event in ["push", "workflow_dispatch", "pull_request", ""] {
        assert!(
            runtime::signing_event(&preview, event).is_err(),
            "accepted {event}"
        );
    }
    let stable = plan(PackageReleaseChannel::Stable);
    assert_eq!(
        runtime::signing_event(&stable, "workflow_dispatch").expect("stable"),
        PackageReleaseEvent::WorkflowDispatch
    );
    assert!(runtime::signing_event(&stable, "workflow_run").is_err());
}

#[test]
fn version_identity_uses_full_commit_count_and_preview_tag() {
    let preview = plan(PackageReleaseChannel::Preview);
    let facts = identity(&preview);
    assert_eq!(
        release_version(&preview, &facts).expect("preview identity"),
        (
            "1.2.3-preview.7+a12e73a".to_owned(),
            "v1.2.3-preview.7+a12e73a".to_owned(),
            Some(7),
            Some("a12e73a".to_owned()),
        )
    );
}

#[test]
fn stable_identity_keeps_dispatch_and_package_prefixed_tag() {
    let stable = plan(PackageReleaseChannel::Stable);
    let facts = identity(&stable);
    assert!(validate_identity(&stable, &facts).is_ok());
    assert_eq!(
        release_version(&stable, &facts).expect("stable identity"),
        ("1.2.3".to_owned(), "jackin-v1.2.3".to_owned(), None, None)
    );
}
#[test]
fn retained_preview_verification_is_closed_to_generated_tap_operations() {
    for operation in [
        "admit-homebrew-tap-v1",
        "write-homebrew-tap-pr-v1",
        "merge-homebrew-tap-pr-v1",
    ] {
        assert!(super::uses_retained_preview(
            Some("workflow_dispatch"),
            Some(operation)
        ));
        assert!(super::uses_retained_preview(
            Some("schedule"),
            Some(operation)
        ));
        for event in [
            None,
            Some("push"),
            Some("workflow_run"),
            Some("pull_request"),
        ] {
            assert!(!super::uses_retained_preview(event, Some(operation)));
        }
    }
    for operation in [
        None,
        Some("prepare-homebrew-tap-v1"),
        Some("sign-package-release-v1"),
        Some("publish-package-release-v1"),
        Some("recover-package-release-v1"),
        Some("unknown"),
    ] {
        assert!(!super::uses_retained_preview(
            Some("workflow_dispatch"),
            operation
        ));
    }
}
