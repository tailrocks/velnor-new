use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_artifact_channels::{job_outputs, upload_step};
use velnor_actions_workflow_renderer::release_jobs::{
    ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, publish_gate_condition,
    reconcile_gate_condition,
};
use velnor_actions_workflow_renderer::shell_step;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const REPO: &str = "acme/widgets";
const LABEL: &str = "ubuntu-26.04";
const PUBLISH_ENV: &str = "auth-active";
const BOOTSTRAP_ENV: &str = "auth-bootstrap";

/// Extract the exact invalid-workflow payload.
pub(crate) fn invalid(result: Result<(), RenderError>) -> Option<String> {
    match result {
        Err(RenderError::InvalidWorkflow(text)) => Some(text),
        Err(_) | Ok(()) => None,
    }
}

pub(crate) fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: "plan-1".to_owned(),
        repository: REPO.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates-io".to_owned(),
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

pub(crate) fn job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<&str>,
    environment: Option<&str>,
) -> Result<ReleaseJobSpec, RenderError> {
    let step = shell_step(
        "Run",
        vec!["echo".to_owned(), "ok".to_owned()],
        BTreeMap::new(),
    )?;
    Ok(ReleaseJobSpec {
        role,
        display_name: role.job_id().to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::RELEASE,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: condition.map(str::to_owned),
        environment: environment.map(str::to_owned),
        permissions: JobPermissions::expected(role),
        steps: vec![step, upload_step(role)?],
        outputs: job_outputs(role)?,
    })
}

fn jobs_for(
    plan: &BootstrapPlan,
    preparation: bool,
) -> Result<BTreeMap<String, ReleaseJobSpec>, RenderError> {
    let gate = publish_gate_condition(REPO, plan, "main");
    let reconcile = reconcile_gate_condition(REPO, plan, "main");
    let registry = if plan.version.is_some() {
        ReleaseRole::RegistryPublishBootstrap
    } else {
        ReleaseRole::RegistryPublishOidc
    };
    let registry_environment = if plan.version.is_some() {
        BOOTSTRAP_ENV
    } else {
        PUBLISH_ENV
    };
    let mut jobs = BTreeMap::new();
    let entries = base_entries(
        registry,
        gate.as_str(),
        reconcile.as_str(),
        registry_environment,
    );
    for (role, mut needs, condition, environment) in entries {
        if registry == ReleaseRole::RegistryPublishBootstrap {
            for need in &mut needs {
                if *need == ReleaseRole::RegistryPublishOidc.job_id() {
                    *need = ReleaseRole::RegistryPublishBootstrap.job_id();
                }
            }
        }
        jobs.insert(
            role.job_id().to_owned(),
            job(role, &needs, condition, environment)?,
        );
    }
    if preparation {
        jobs.insert(
            ReleaseRole::PreparationAnonymous.job_id().to_owned(),
            job(ReleaseRole::PreparationAnonymous, &[], None, None)?,
        );
        jobs.insert(
            ReleaseRole::PreparationForge.job_id().to_owned(),
            job(
                ReleaseRole::PreparationForge,
                &[ReleaseRole::PreparationAnonymous.job_id()],
                Some(gate.as_str()),
                Some(PUBLISH_ENV),
            )?,
        );
    }
    Ok(jobs)
}

fn base_entries<'a>(
    registry: ReleaseRole,
    gate: &'a str,
    reconcile: &'a str,
    environment: &'a str,
) -> Vec<(
    ReleaseRole,
    Vec<&'static str>,
    Option<&'a str>,
    Option<&'a str>,
)> {
    vec![
        (ReleaseRole::PackageAnonymous, vec![], None, None),
        (
            ReleaseRole::PreflightForge,
            vec![ReleaseRole::PackageAnonymous.job_id()],
            None,
            None,
        ),
        (
            registry,
            vec![
                ReleaseRole::PackageAnonymous.job_id(),
                ReleaseRole::PreflightForge.job_id(),
            ],
            Some(gate),
            Some(environment),
        ),
        (
            ReleaseRole::ForgePublish,
            vec![
                ReleaseRole::PackageAnonymous.job_id(),
                ReleaseRole::PreflightForge.job_id(),
                ReleaseRole::RegistryPublishOidc.job_id(),
            ],
            Some(gate),
            Some(environment),
        ),
        (
            ReleaseRole::Reconcile,
            vec![
                ReleaseRole::PackageAnonymous.job_id(),
                ReleaseRole::PreflightForge.job_id(),
                ReleaseRole::RegistryPublishOidc.job_id(),
                ReleaseRole::ForgePublish.job_id(),
            ],
            Some(reconcile),
            None,
        ),
    ]
}

fn workflow(version: Option<&str>, preparation: bool) -> Result<ReleaseWorkflowSpec, RenderError> {
    let mut plan = bootstrap();
    plan.version = version.map(str::to_owned);
    let mut dispatch_inputs = vec![
        bound_input("plan", &plan.plan_id),
        bound_input("source_sha", SHA),
    ];
    if let Some(version) = &plan.version {
        dispatch_inputs.push(bound_input("version", version));
    }
    Ok(ReleaseWorkflowSpec {
        name: "Velnor Release".to_owned(),
        repository: REPO.to_owned(),
        triggers: ReleaseTriggers {
            push_branches: vec!["main".to_owned()],
            schedule: None,
            dispatch_inputs,
        },
        concurrency: ReleaseConcurrency {
            group: "release-acme/widgets".to_owned(),
            cancel_in_progress: false,
        },
        jobs: jobs_for(&plan, preparation)?,
        preparation_enabled: preparation,
        helper_registry: super::impl_renderer_release_gates::bootstrap_fixture::helper_registry(),
        support_sources: super::impl_renderer_release_gates::bootstrap_fixture::support_sources(),
        bootstrap_tools: super::impl_renderer_release_gates::bootstrap_tools(),
        reconciliation: authority::reconciliation(&plan),
        bootstrap: plan,
        publish_environment: PUBLISH_ENV.to_owned(),
        bootstrap_environment: BOOTSTRAP_ENV.to_owned(),
    })
}

pub(crate) fn spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    workflow(None, false)
}

pub(crate) fn preparation_spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    workflow(None, true)
}

pub(crate) fn bootstrap_spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    workflow(Some("1.2.3"), false)
}

#[test]
fn role_names_and_ids_are_closed() {
    let expected = [
        (
            ReleaseRole::SourceSnapshotForge,
            "source-snapshot-forge",
            "release-source-snapshot",
            true,
        ),
        (
            ReleaseRole::PackageAnonymous,
            "package-anonymous",
            "release-package",
            true,
        ),
        (
            ReleaseRole::PreflightForge,
            "preflight-forge",
            "release-preflight",
            true,
        ),
        (
            ReleaseRole::RegistryPublishOidc,
            "registry-publish-oidc",
            "release-registry-publish",
            false,
        ),
        (
            ReleaseRole::RegistryPublishBootstrap,
            "registry-publish-bootstrap",
            "release-registry-publish",
            false,
        ),
        (
            ReleaseRole::ForgePublish,
            "forge-publish",
            "release-forge-publish",
            false,
        ),
        (
            ReleaseRole::Reconcile,
            "reconcile",
            "release-reconcile",
            true,
        ),
        (
            ReleaseRole::PreparationAnonymous,
            "preparation-anonymous",
            "release-preparation-source",
            true,
        ),
        (
            ReleaseRole::PreparationForge,
            "preparation-forge",
            "release-preparation",
            false,
        ),
    ];
    for (role, name, id, validation) in expected {
        assert_eq!(role.as_str(), name);
        assert_eq!(role.job_id(), id);
        assert_eq!(role.is_validation(), validation);
    }
}

#[test]
fn job_shape_rejects_malformed_jobs() -> Result<(), RenderError> {
    let mut empty = job(ReleaseRole::PreflightForge, &[], None, None)?;
    empty.steps.clear();
    assert!(invalid(empty.validate_shape(ReleaseRole::PreflightForge.job_id())).is_some());
    let mut label = job(ReleaseRole::PreflightForge, &[], None, None)?;
    for bad in ["ubuntu-latest", "self-hosted", "macos-15"] {
        label.runs_on = bad.to_owned();
        assert!(invalid(label.validate_shape(label.role.job_id())).is_some());
    }
    let mut display = job(ReleaseRole::PreflightForge, &[], None, None)?;
    display.display_name = "bad ${{ x }}".to_owned();
    assert!(invalid(display.validate_shape(display.role.job_id())).is_some());
    Ok(())
}

#[path = "impl_renderer_release_authority.rs"]
pub(crate) mod authority;

#[path = "impl_renderer_release_graph.rs"]
mod graph;
