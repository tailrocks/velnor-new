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
fn consumer_gate_rejects_unsafe_identity_and_workflow_paths() {
    for repository in ["example", "example/repo;scan", "example/repo/extra"] {
        let mut invalid = support::context();
        invalid.repository = repository.to_owned();
        assert!(consumer_script(&invalid, &["gh".to_owned()], false).is_err());
    }
    let mut invalid: ReleaseEligibilityContext = support::context();
    invalid.workflow_path = ".github/workflows/other.yml".to_owned();
    assert!(consumer_script(&invalid, &["gh".to_owned()], false).is_err());
}
