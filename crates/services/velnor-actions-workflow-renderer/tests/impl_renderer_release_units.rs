//! Release split parity: matrix, validators, and job-shape roundtrips.
//!
//! Executes the code moved into `release_permissions` and the
//! `release_spec::validators` child module plus the `PartialEq`-free
//! `ReleaseJobSpec`, pinning pre-split behavior through public paths.
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_jobs::{
    ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::{JobPermissions, PermissionLevel};
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, ReleaseConcurrency, ReleaseTriggers, validate_environment,
    validate_package_name, validate_package_version, validate_plan_id, validate_repository,
    validate_source_sha,
};

#[test]
fn permission_matrix_matches_documented_roles() -> Result<(), RenderError> {
    let matrix = [
        (
            ReleaseRole::Preparation,
            PermissionLevel::Write,
            PermissionLevel::Write,
            PermissionLevel::None,
        ),
        (
            ReleaseRole::Preflight,
            PermissionLevel::Read,
            PermissionLevel::None,
            PermissionLevel::None,
        ),
        (
            ReleaseRole::PublishOidc,
            PermissionLevel::Write,
            PermissionLevel::Read,
            PermissionLevel::Write,
        ),
        (
            ReleaseRole::PublishBootstrap,
            PermissionLevel::Write,
            PermissionLevel::Read,
            PermissionLevel::None,
        ),
        (
            ReleaseRole::Reconcile,
            PermissionLevel::Read,
            PermissionLevel::Read,
            PermissionLevel::None,
        ),
    ];
    for (role, contents, pulls, token) in matrix {
        let got = JobPermissions::expected(role);
        assert_eq!(got.contents, contents, "{}", role.as_str());
        assert_eq!(got.pull_requests, pulls, "{}", role.as_str());
        assert_eq!(got.id_token, token, "{}", role.as_str());
        let env = if matches!(token, PermissionLevel::Write) {
            Some("release")
        } else {
            None
        };
        got.validate(role, env)?;
    }
    Ok(())
}

#[test]
fn permission_matrix_rejects_drift_and_missing_environment() {
    let oidc = JobPermissions::expected(ReleaseRole::PublishOidc);
    assert!(oidc.validate(ReleaseRole::PublishOidc, None).is_err());
    assert!(oidc.validate(ReleaseRole::Preflight, None).is_err());
    let read = JobPermissions::expected(ReleaseRole::Preflight);
    assert!(
        read.validate(ReleaseRole::PublishOidc, Some("release"))
            .is_err()
    );
}

#[test]
fn scalar_validators_accept_and_reject() -> Result<(), RenderError> {
    validate_environment("release")?;
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
    let shell = Step {
        name: "Check out".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let job = ReleaseJobSpec {
        role: ReleaseRole::Preflight,
        display_name: "Preflight".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::RELEASE,
        needs: vec!["preparation".to_owned()],
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preflight),
        steps: vec![shell],
    };
    job.validate_shape("preflight")?;
    let internal = ReleaseJobSpec {
        steps: vec![Step {
            name: "Plan".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Internal {
                operation: "plan-v1".to_owned(),
                env: std::collections::BTreeMap::new(),
            },
        }],
        ..job.clone()
    };
    assert!(internal.validate_shape("preflight").is_err());
    Ok(())
}

#[test]
fn release_workflow_spec_rejects_bad_identity() {
    let spec = ReleaseWorkflowSpec {
        name: String::new(),
        repository: "tailrocks/velnor-new".to_owned(),
        triggers: ReleaseTriggers {
            push_branches: vec!["main".to_owned()],
            schedule: None,
            dispatch_inputs: Vec::new(),
        },
        concurrency: ReleaseConcurrency {
            group: "lock".to_owned(),
            cancel_in_progress: false,
        },
        jobs: BTreeMap::new(),
        bootstrap: BootstrapPlan {
            plan_id: "plan-r1-a1".to_owned(),
            repository: "tailrocks/velnor-new".to_owned(),
            source_sha: "a".repeat(40),
            registry: "crates-io".to_owned(),
            packages: BTreeMap::from([("demo".to_owned(), "0.1.0".to_owned())]),
            version: None,
        },
        publish_environment: "release".to_owned(),
        bootstrap_environment: "release-bootstrap".to_owned(),
    };
    let err = spec.validate().expect_err("empty name must fail");
    assert!(err.to_string().contains("bad_release_name"), "{err}");
}
