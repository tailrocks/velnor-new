use super::{EnsureError, require_group_policy_evidence, validate_binding, validate_group};
use crate::{RegistrationScopeKind, ScaleSetBinding};
use velnor_runner_github::RunnerGroup;

fn binding() -> ScaleSetBinding {
    ScaleSetBinding {
        scope: RegistrationScopeKind::Repository,
        owner: "ChainArgos".to_owned(),
        repository: "java-monorepo".to_owned(),
        scale_set_name: "ubuntu-26.04-scale-set".to_owned(),
        runner_group_id: 1,
        runner_group_name: "Default".to_owned(),
        runner_image_profile: None,
    }
}

#[test]
fn registration_requires_the_exact_scope_group_and_supported_profile() {
    assert!(validate_binding("token", &binding()).is_ok());

    let mut mismatch = binding();
    mismatch.scale_set_name = "ubuntu-24.04-scale-set".to_owned();
    assert_eq!(
        validate_binding("token", &mismatch),
        Err(EnsureError::Rejected)
    );

    let mut invalid_group = binding();
    invalid_group.runner_group_id = 0;
    assert_eq!(
        validate_binding("token", &invalid_group),
        Err(EnsureError::Rejected)
    );
    assert_eq!(validate_binding("", &binding()), Err(EnsureError::Rejected));
}

#[test]
fn group_resolution_requires_one_exact_id_and_name_match() {
    let binding = binding();
    let groups = [RunnerGroup {
        id: 1,
        name: "Default".to_owned(),
        is_default: true,
    }];
    assert!(validate_group(&groups, &binding).is_ok());

    let wrong_id = [RunnerGroup {
        id: 2,
        name: "Default".to_owned(),
        is_default: true,
    }];
    assert_eq!(
        validate_group(&wrong_id, &binding),
        Err(EnsureError::Rejected)
    );

    let ambiguous_name = [
        RunnerGroup {
            id: 1,
            name: "Default".to_owned(),
            is_default: true,
        },
        RunnerGroup {
            id: 2,
            name: "Default".to_owned(),
            is_default: false,
        },
    ];
    assert_eq!(
        validate_group(&ambiguous_name, &binding),
        Err(EnsureError::Rejected)
    );
}

#[test]
fn linux_binding_is_rejected_when_scope_policy_evidence_is_missing() {
    let mut linux = binding();
    linux.scale_set_name = "ubuntu-24.04-scale-set".to_owned();
    linux.runner_image_profile = Some("ubuntu-24.04-amd64".to_owned());

    assert!(validate_binding("credential", &linux).is_ok());
    assert_eq!(
        require_group_policy_evidence(&linux),
        Err(EnsureError::GroupPolicyUnavailable)
    );
    assert_eq!(require_group_policy_evidence(&binding()), Ok(()));
}
