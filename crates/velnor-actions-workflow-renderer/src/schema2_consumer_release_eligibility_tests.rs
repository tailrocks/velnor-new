use super::{ReleaseEligibilityContext, consumer_script};

#[path = "schema2_consumer_release_eligibility_behavior_tests.rs"]
mod behavior;
#[path = "schema2_consumer_release_eligibility_test_support.rs"]
mod support;

#[test]
fn consumer_gate_binds_repository_branch_and_workflow_authority() {
    let script =
        consumer_script(&support::context(), &["gh".to_owned()], false).expect("valid context");
    assert!(script.contains("repository='example/repo-scan'"));
    assert!(script.contains("default_branch='stable'"));
    assert!(script.contains("refs/heads/$default_branch"));
    assert!(script.contains("binary-release.yml@refs/heads/$default_branch"));
    assert!(script.contains("latest CI attempt changed during eligibility check"));
    assert!(!script.contains("tailrocks/velnor-new"));
}

#[test]
fn consumer_gate_rejects_unsafe_identity_and_derives_channel_authority() {
    for repository in ["example", "example/repo;scan", "example/repo/extra"] {
        let mut invalid = support::context();
        invalid.repository = repository.to_owned();
        assert!(consumer_script(&invalid, &["gh".to_owned()], false).is_err());
    }
    let mut preview: ReleaseEligibilityContext = support::context();
    preview.channel = velnor_actions_contract::package_release::PackageReleaseChannel::Preview;
    let script = consumer_script(&preview, &["gh".to_owned()], false).expect("native preview");
    assert!(script.contains("binary-preview.yml@refs/heads/$default_branch"));
    assert!(script.contains("workflow_run|workflow_dispatch) ;;"));
    let stable = consumer_script(&support::context(), &["gh".to_owned()], false).expect("stable");
    assert!(stable.contains("  workflow_dispatch) ;;"));
    assert!(!stable.contains("workflow_run|workflow_dispatch) ;;"));
}

#[path = "schema2_preview_eligibility_behavior_tests.rs"]
mod preview_behavior;

#[path = "schema2_preview_completion_admission_tests.rs"]
mod completion_admission;
