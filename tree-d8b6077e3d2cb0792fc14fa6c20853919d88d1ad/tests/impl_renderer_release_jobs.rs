//! Release role, permission, shape, and workflow-graph cases.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::release_jobs::{
    ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::{JobPermissions, PermissionLevel};
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, publish_gate_condition,
};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_steps::shell_step;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const REPO: &str = "acme/widgets";
const LABEL: &str = "ubuntu-24.04";
const ENV: &str = "crates-io";

/// Extract the `InvalidWorkflow` payload; `None` unless the exact rejection fired.
fn invalid(result: Result<(), RenderError>) -> Option<String> {
    match result {
        Err(RenderError::InvalidWorkflow(text)) => Some(text),
        Err(_) | Ok(()) => None,
    }
}

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: "plan-1".to_owned(),
        repository: REPO.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates_io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
        version: None,
    }
}

fn bound_input(name: &str, default: &str) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        description: format!("approved {name}"),
        required: true,
        default: Some(default.to_owned()),
    }
}

fn job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<&str>,
    env: Option<&str>,
) -> Result<ReleaseJobSpec, RenderError> {
    let step = shell_step(
        "Run",
        vec!["echo".to_owned(), "ok".to_owned()],
        BTreeMap::new(),
    )?;
    Ok(ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        timeout_minutes: velnor_actions_contract_workflow::JobTimeout::RELEASE,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: condition.map(str::to_owned),
        environment: env.map(str::to_owned),
        permissions: JobPermissions::expected(role),
        steps: vec![step],
    })
}

fn spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    let gate = publish_gate_condition(REPO, &bootstrap());
    let jobs = BTreeMap::from([
        (
            "release-preparation".to_owned(),
            job(ReleaseRole::Preparation, &[], None, None)?,
        ),
        (
            "release-preflight".to_owned(),
            job(ReleaseRole::Preflight, &["release-preparation"], None, None)?,
        ),
        (
            "release-publish".to_owned(),
            job(
                ReleaseRole::PublishOidc,
                &["release-preflight"],
                Some(&gate),
                Some(ENV),
            )?,
        ),
        (
            "release-reconcile".to_owned(),
            job(
                ReleaseRole::Reconcile,
                &["release-publish"],
                Some("always()"),
                None,
            )?,
        ),
    ]);
    Ok(ReleaseWorkflowSpec {
        name: "Velnor Release".to_owned(),
        repository: REPO.to_owned(),
        triggers: ReleaseTriggers {
            push_branches: vec!["main".to_owned()],
            schedule: None,
            dispatch_inputs: vec![
                bound_input("plan", "plan-1"),
                bound_input("source_sha", SHA),
            ],
        },
        concurrency: ReleaseConcurrency {
            group: "release-acme/widgets".to_owned(),
            cancel_in_progress: false,
        },
        jobs,
        bootstrap: bootstrap(),
        publish_environment: ENV.to_owned(),
        bootstrap_environment: "crates-io-bootstrap".to_owned(),
    })
}

#[test]
fn role_names_validation_and_matrix_are_exact() {
    let names = [
        (ReleaseRole::Preparation, "preparation", false),
        (ReleaseRole::Preflight, "preflight", true),
        (ReleaseRole::PublishOidc, "publish-oidc", false),
        (ReleaseRole::PublishBootstrap, "publish-bootstrap", false),
        (ReleaseRole::Reconcile, "reconcile", true),
    ];
    for (role, name, validation) in names {
        assert_eq!(role.as_str(), name);
        assert_eq!(role.is_validation(), validation, "for {name}");
    }
    let prep = JobPermissions::expected(ReleaseRole::Preparation);
    assert_eq!(prep.contents, PermissionLevel::Write);
    assert_eq!(prep.pull_requests, PermissionLevel::Write);
    assert_eq!(prep.id_token, PermissionLevel::None);
    let oidc = JobPermissions::expected(ReleaseRole::PublishOidc);
    assert_eq!(oidc.id_token, PermissionLevel::Write);
    let token = JobPermissions::expected(ReleaseRole::PublishBootstrap);
    assert_eq!(token.id_token, PermissionLevel::None);
    assert_eq!(token.contents, PermissionLevel::Write);
    let flight = JobPermissions::expected(ReleaseRole::Preflight);
    assert_eq!(flight.contents, PermissionLevel::Read);
    assert_eq!(flight.pull_requests, PermissionLevel::None);
}

#[test]
fn permission_rules_bind_oidc_and_validation_roles() {
    let oidc = JobPermissions::expected(ReleaseRole::PublishOidc);
    assert!(oidc.validate(ReleaseRole::PublishOidc, Some(ENV)).is_ok());
    assert_eq!(
        invalid(oidc.validate(ReleaseRole::PublishOidc, None)).expect("reject"),
        "id_token_without_environment"
    );
    let elevated = JobPermissions {
        contents: PermissionLevel::Write,
        ..JobPermissions::expected(ReleaseRole::Preflight)
    };
    assert_eq!(
        invalid(elevated.validate(ReleaseRole::Preflight, None)).expect("reject"),
        "contents_write_on_validation"
    );
    let drifted = JobPermissions {
        pull_requests: PermissionLevel::Write,
        ..JobPermissions::expected(ReleaseRole::Reconcile)
    };
    assert_eq!(
        invalid(drifted.validate(ReleaseRole::Reconcile, None)).expect("reject"),
        "permission_matrix:reconcile"
    );
}

#[test]
fn shape_rejects_malformed_jobs() -> Result<(), RenderError> {
    let mut empty = job(ReleaseRole::Preflight, &[], None, None)?;
    empty.steps.clear();
    assert!(
        invalid(empty.validate_shape("release-preflight"))
            .expect("reject")
            .starts_with("empty_steps:")
    );
    let mut label = job(ReleaseRole::Preflight, &[], None, None)?;
    for bad in [
        "ubuntu-latest",
        "self-hosted",
        "macos-15",
        "ubuntu-${{ matrix.os }}",
    ] {
        label.runs_on = bad.to_owned();
        assert!(
            invalid(label.validate_shape("release-preflight"))
                .expect("reject")
                .starts_with("unpinned_label:")
        );
    }
    let mut display = job(ReleaseRole::Preflight, &[], None, None)?;
    display.display_name = "bad ${{ x }}".to_owned();
    assert!(
        invalid(display.validate_shape("release-preflight"))
            .expect("reject")
            .starts_with("bad_job_display:")
    );
    let mut secreted = job(ReleaseRole::PublishOidc, &[], None, Some(ENV))?;
    secreted.condition = Some("secrets.TOKEN != ''".to_owned());
    assert!(
        invalid(secreted.validate_shape("release-publish"))
            .expect("reject")
            .starts_with("bad_condition:")
    );
    Ok(())
}

#[test]
fn spec_accepts_four_roles_and_optional_bootstrap() -> Result<(), RenderError> {
    assert!(spec()?.validate().is_ok());
    let mut with_token = spec()?;
    let gate = publish_gate_condition(REPO, &bootstrap());
    with_token.jobs.insert(
        "release-bootstrap".to_owned(),
        job(
            ReleaseRole::PublishBootstrap,
            &["release-preflight"],
            Some(&gate),
            Some(ENV),
        )?,
    );
    with_token
        .jobs
        .get_mut("release-reconcile")
        .expect("reconcile")
        .needs
        .push("release-bootstrap".to_owned());
    assert!(with_token.validate().is_ok());
    Ok(())
}

#[test]
fn role_set_rejects_gaps_and_duplicates() -> Result<(), RenderError> {
    let mut gap = spec()?;
    gap.jobs.remove("release-reconcile").expect("remove");
    assert!(
        invalid(gap.validate())
            .expect("reject")
            .starts_with("release_role_set:")
    );
    let mut duplicate = spec()?;
    duplicate.jobs.insert(
        "release-extra".to_owned(),
        job(ReleaseRole::Preparation, &[], None, None)?,
    );
    assert!(
        invalid(duplicate.validate())
            .expect("reject")
            .starts_with("release_role_set:")
    );
    let mut branded = spec()?;
    let held = branded.jobs.remove("release-preflight").expect("remove");
    branded.jobs.insert("velnor-preflight".to_owned(), held);
    assert!(branded.validate().is_err(), "branded ids stay reserved");
    Ok(())
}

#[test]
fn needs_graph_rejects_unknown_self_and_backward_edges() -> Result<(), RenderError> {
    let mut unknown = spec()?;
    unknown
        .jobs
        .get_mut("release-preflight")
        .expect("preflight")
        .needs
        .push("ghost".to_owned());
    assert!(
        invalid(unknown.validate())
            .expect("reject")
            .starts_with("unknown_need:")
    );
    let mut looping = spec()?;
    looping
        .jobs
        .get_mut("release-preflight")
        .expect("preflight")
        .needs = vec!["release-preflight".to_owned()];
    assert!(
        invalid(looping.validate())
            .expect("reject")
            .starts_with("self_need:")
    );
    let mut backward = spec()?;
    backward
        .jobs
        .get_mut("release-preflight")
        .expect("preflight")
        .needs = vec!["release-publish".to_owned()];
    assert!(
        invalid(backward.validate())
            .expect("reject")
            .starts_with("backward_need:")
    );
    Ok(())
}

#[test]
fn publish_and_reconcile_conditions_are_exact() -> Result<(), RenderError> {
    let mut loose = spec()?;
    loose
        .jobs
        .get_mut("release-publish")
        .expect("publish")
        .condition = Some("true".to_owned());
    assert!(
        invalid(loose.validate())
            .expect("reject")
            .starts_with("publish_gate_mismatch:")
    );
    let mut missing = spec()?;
    missing
        .jobs
        .get_mut("release-publish")
        .expect("publish")
        .condition = None;
    assert!(
        invalid(missing.validate())
            .expect("reject")
            .starts_with("publish_gate_mismatch:")
    );
    let mut lazy = spec()?;
    lazy.jobs
        .get_mut("release-reconcile")
        .expect("reconcile")
        .condition = Some("success()".to_owned());
    assert!(
        invalid(lazy.validate())
            .expect("reject")
            .starts_with("reconcile_condition:")
    );
    Ok(())
}

#[test]
fn publish_follows_preflight_and_reconcile_follows_publish() -> Result<(), RenderError> {
    let mut direct = spec()?;
    direct
        .jobs
        .get_mut("release-publish")
        .expect("publish")
        .needs
        .clear();
    assert!(
        invalid(direct.validate())
            .expect("reject")
            .starts_with("publish_without_preflight:")
    );
    let mut early = spec()?;
    early
        .jobs
        .get_mut("release-reconcile")
        .expect("reconcile")
        .needs
        .clear();
    assert!(
        invalid(early.validate())
            .expect("reject")
            .starts_with("reconcile_without_publish:")
    );
    Ok(())
}

#[test]
fn bootstrap_repository_mismatch_fails_closed() -> Result<(), RenderError> {
    let mut forked = spec()?;
    forked.bootstrap.repository = "mallory/widgets".to_owned();
    assert_eq!(
        invalid(forked.validate()).expect("reject"),
        "bootstrap_repository_mismatch"
    );
    Ok(())
}
