//! Release role permission and typed IR cases.
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_artifact_channels::{job_outputs, upload_step};
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_permissions::{JobPermissions, PermissionLevel};
use velnor_actions_workflow_renderer::release_spec::{
    validate_environment, validate_package_name, validate_package_version, validate_plan_id,
    validate_repository, validate_source_sha,
};

#[test]
fn permission_matrix_matches_all_closed_roles() -> Result<(), RenderError> {
    use PermissionLevel::{None, Read, Write};

    let matrix = [
        (ReleaseRole::SourceSnapshotForge, Read, Read, None, None),
        (ReleaseRole::PackageAnonymous, Read, None, None, None),
        (ReleaseRole::PreflightForge, Read, Read, None, None),
        (ReleaseRole::RegistryPublishOidc, Read, Read, None, Write),
        (
            ReleaseRole::RegistryPublishBootstrap,
            Read,
            Read,
            None,
            None,
        ),
        (ReleaseRole::ForgePublish, Write, Read, None, None),
        (ReleaseRole::Reconcile, Read, Read, None, None),
        (ReleaseRole::PreparationAnonymous, Read, None, None, None),
        (ReleaseRole::PreparationForge, Write, Read, Write, None),
    ];
    for (role, contents, actions, pull_requests, id_token) in matrix {
        let permissions = JobPermissions::expected(role);
        assert_eq!(permissions.contents, contents, "{}", role.as_str());
        assert_eq!(permissions.actions, actions, "{}", role.as_str());
        assert_eq!(
            permissions.pull_requests,
            pull_requests,
            "{}",
            role.as_str()
        );
        assert_eq!(permissions.id_token, id_token, "{}", role.as_str());
        let environment = (id_token == Write).then_some("auth-active");
        permissions.validate(role, environment)?;
    }
    Ok(())
}

#[test]
fn permission_matrix_rejects_auth_and_write_drift() {
    let mut registry = JobPermissions::expected(ReleaseRole::RegistryPublishOidc);
    registry.contents = PermissionLevel::Write;
    assert!(
        registry
            .validate(ReleaseRole::RegistryPublishOidc, Some("auth-active"))
            .is_err()
    );

    let mut forge = JobPermissions::expected(ReleaseRole::ForgePublish);
    forge.pull_requests = PermissionLevel::Write;
    assert!(
        forge
            .validate(ReleaseRole::ForgePublish, Some("auth-active"))
            .is_err()
    );

    let mut preparation = JobPermissions::expected(ReleaseRole::PreparationForge);
    preparation.pull_requests = PermissionLevel::None;
    assert!(
        preparation
            .validate(ReleaseRole::PreparationForge, Some("auth-active"))
            .is_err()
    );

    let mut anonymous = JobPermissions::expected(ReleaseRole::PackageAnonymous);
    anonymous.contents = PermissionLevel::Write;
    assert!(
        anonymous
            .validate(ReleaseRole::PackageAnonymous, None)
            .is_err()
    );

    let oidc = JobPermissions::expected(ReleaseRole::RegistryPublishOidc);
    assert!(
        oidc.validate(ReleaseRole::RegistryPublishOidc, None)
            .is_err()
    );
    let bootstrap = JobPermissions::expected(ReleaseRole::RegistryPublishBootstrap);
    assert!(
        bootstrap
            .validate(ReleaseRole::RegistryPublishBootstrap, None)
            .is_ok()
    );
}

#[test]
fn scalar_validators_accept_and_reject() -> Result<(), RenderError> {
    validate_environment("auth-active")?;
    assert!(validate_environment("has space").is_err());
    validate_repository("tailrocks/velnor-new")?;
    assert!(validate_repository("no-slash").is_err());
    validate_source_sha(&"a".repeat(40))?;
    assert!(validate_source_sha("short").is_err());
    validate_plan_id("plan-r1-a1")?;
    assert!(validate_plan_id("").is_err());
    validate_package_name("velnor-actions-cli")?;
    assert!(validate_package_name("-bad").is_err());
    validate_package_version("0.3.169")?;
    assert!(validate_package_version("1.2").is_err());
    Ok(())
}

#[test]
fn release_job_shape_roundtrip() -> Result<(), RenderError> {
    let role = ReleaseRole::PreflightForge;
    let shell = Step {
        id: None,
        name: "Check out".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let job = ReleaseJobSpec {
        role,
        display_name: role.job_id().to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::RELEASE,
        needs: vec![ReleaseRole::PackageAnonymous.job_id().to_owned()],
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(role),
        steps: vec![shell, upload_step(role)?],
        outputs: job_outputs(role)?,
    };
    job.validate_shape(role.job_id())?;
    let internal = ReleaseJobSpec {
        steps: vec![Step {
            id: None,
            name: "Plan".to_owned(),
            condition: None,
            kind: StepKind::Internal {
                operation: "plan-v1".to_owned(),
            },
        }],
        ..job
    };
    assert!(internal.validate_shape(role.job_id()).is_err());
    Ok(())
}

#[test]
fn workflow_spec_uses_fixture_records_and_rejects_bad_identity() -> Result<(), RenderError> {
    let mut workflow = super::impl_renderer_release_jobs::spec()?;
    workflow.name.clear();
    let error = workflow.validate().expect_err("empty name must fail");
    assert!(error.to_string().contains("bad_release_name"), "{error}");
    Ok(())
}
