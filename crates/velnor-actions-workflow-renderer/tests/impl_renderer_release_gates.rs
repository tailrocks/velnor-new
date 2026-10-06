//! Release execution-gate fixtures and structural regressions.
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepId, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::action_step;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::{
    ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, publish_gate_condition,
    reconcile_gate_condition,
};

#[path = "impl_renderer_release_bootstrap_fixture.rs"]
pub(crate) mod bootstrap_fixture;
#[path = "impl_renderer_release_bootstrap.rs"]
mod bootstrap_tests;
#[path = "impl_renderer_release_gate_fixture.rs"]
pub(crate) mod fixture;
#[path = "impl_renderer_release_gate_order.rs"]
mod order_tests;
#[path = "impl_renderer_release_prepared.rs"]
mod prepared_tests;
#[path = "impl_renderer_release_source.rs"]
mod source_tests;

pub(crate) const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
pub(crate) const OTHER_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
pub(crate) const REPO: &str = "acme/widgets";
pub(crate) const LABEL: &str = "ubuntu-26.04";
pub(crate) const ENV: &str = "crates-io";
pub(crate) const BOOTSTRAP_ENV: &str = "crates-io-bootstrap";

pub(crate) fn invalid(result: Result<(), RenderError>) -> Option<String> {
    match result {
        Err(RenderError::InvalidWorkflow(text)) => Some(text),
        Err(_) | Ok(()) => None,
    }
}

pub(crate) fn checkout_uses() -> String {
    format!("actions/checkout@{:040x}", 0)
}

pub(crate) fn checkout(with: &[(&str, &str)]) -> Result<Step, RenderError> {
    action_step(
        "Checkout exact source",
        &checkout_uses(),
        with.iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    )
}

pub(crate) fn source_checkout(sha: &str, persist: Option<&str>) -> Result<Step, RenderError> {
    let mut inputs = vec![
        ("fetch-depth", "0"),
        ("path", fixture::source_dir()),
        ("ref", sha),
    ];
    if let Some(value) = persist {
        inputs.push(("persist-credentials", value));
    }
    checkout(&inputs)
}

pub(crate) fn raw_shell(name: &str) -> Step {
    Step {
        id: None,
        name: name.to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["echo".to_owned(), "forged".to_owned()],
            env: BTreeMap::new(),
        },
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

fn job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<&str>,
    environment: Option<&str>,
    steps: Vec<Step>,
) -> Result<ReleaseJobSpec, RenderError> {
    Ok(ReleaseJobSpec {
        role,
        display_name: role.job_id().to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::RELEASE,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: condition.map(str::to_owned),
        environment: environment.map(str::to_owned),
        permissions: JobPermissions::expected(role),
        steps,
        outputs: velnor_actions_workflow_renderer::release_artifact_channels::job_outputs(role)?,
    })
}

fn inputs() -> Vec<DispatchInput> {
    vec![
        DispatchInput {
            name: "plan".to_owned(),
            description: "approved plan".to_owned(),
            required: true,
            default: Some("plan-1".to_owned()),
        },
        DispatchInput {
            name: "source_sha".to_owned(),
            description: "approved source".to_owned(),
            required: true,
            default: Some(SHA.to_owned()),
        },
    ]
}

pub(crate) fn workflow(
    bootstrap_mode: bool,
    preparation_enabled: bool,
) -> Result<ReleaseWorkflowSpec, RenderError> {
    let mut plan = bootstrap();
    if bootstrap_mode {
        plan.version = Some("1.2.3".to_owned());
    }
    let gate = publish_gate_condition(REPO, &plan, "main");
    let reconcile = reconcile_gate_condition(REPO, &plan, "main");
    let registry_role = if bootstrap_mode {
        ReleaseRole::RegistryPublishBootstrap
    } else {
        ReleaseRole::RegistryPublishOidc
    };
    let registry_environment = if bootstrap_mode { BOOTSTRAP_ENV } else { ENV };
    let mut jobs = base_jobs(
        bootstrap_mode,
        &gate,
        &reconcile,
        registry_role,
        registry_environment,
    )?;
    if preparation_enabled {
        add_preparation_jobs(&mut jobs, bootstrap_mode, &gate)?;
    }
    let mut dispatch_inputs = inputs();
    if bootstrap_mode {
        dispatch_inputs.push(DispatchInput {
            name: "version".to_owned(),
            description: "approved version".to_owned(),
            required: true,
            default: plan.version.clone(),
        });
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
        jobs,
        preparation_enabled,
        helper_registry: fixture::helper_registry(bootstrap_mode),
        support_sources: bootstrap_fixture::support_sources(),
        bootstrap_tools: bootstrap_tools(),
        bootstrap: plan.clone(),
        reconciliation: super::impl_renderer_release_jobs::authority::reconciliation(&plan),
        publish_environment: ENV.to_owned(),
        bootstrap_environment: BOOTSTRAP_ENV.to_owned(),
    })
}

fn base_jobs(
    bootstrap_mode: bool,
    gate: &str,
    reconcile: &str,
    registry_role: ReleaseRole,
    registry_environment: &str,
) -> Result<BTreeMap<String, ReleaseJobSpec>, RenderError> {
    Ok(BTreeMap::from([
        (
            ReleaseRole::PackageAnonymous.job_id().to_owned(),
            job(
                ReleaseRole::PackageAnonymous,
                &[],
                None,
                None,
                fixture::package(bootstrap_mode)?,
            )?,
        ),
        (
            ReleaseRole::PreflightForge.job_id().to_owned(),
            job(
                ReleaseRole::PreflightForge,
                &[ReleaseRole::PackageAnonymous.job_id()],
                None,
                None,
                fixture::preflight(bootstrap_mode)?,
            )?,
        ),
        (
            registry_role.job_id().to_owned(),
            job(
                registry_role,
                &[
                    ReleaseRole::PackageAnonymous.job_id(),
                    ReleaseRole::PreflightForge.job_id(),
                ],
                Some(gate),
                Some(registry_environment),
                fixture::publish(bootstrap_mode)?,
            )?,
        ),
        (
            ReleaseRole::ForgePublish.job_id().to_owned(),
            job(
                ReleaseRole::ForgePublish,
                &[
                    ReleaseRole::PackageAnonymous.job_id(),
                    ReleaseRole::PreflightForge.job_id(),
                    registry_role.job_id(),
                ],
                Some(gate),
                Some(registry_environment),
                fixture::forge(bootstrap_mode)?,
            )?,
        ),
        (
            ReleaseRole::Reconcile.job_id().to_owned(),
            job(
                ReleaseRole::Reconcile,
                &[
                    ReleaseRole::PackageAnonymous.job_id(),
                    ReleaseRole::PreflightForge.job_id(),
                    registry_role.job_id(),
                    ReleaseRole::ForgePublish.job_id(),
                ],
                Some(reconcile),
                None,
                fixture::reconcile(bootstrap_mode)?,
            )?,
        ),
    ]))
}

fn add_preparation_jobs(
    jobs: &mut BTreeMap<String, ReleaseJobSpec>,
    bootstrap_mode: bool,
    gate: &str,
) -> Result<(), RenderError> {
    jobs.insert(
        ReleaseRole::PreparationAnonymous.job_id().to_owned(),
        job(
            ReleaseRole::PreparationAnonymous,
            &[],
            None,
            None,
            fixture::preparation_source(bootstrap_mode)?,
        )?,
    );
    jobs.insert(
        ReleaseRole::PreparationForge.job_id().to_owned(),
        job(
            ReleaseRole::PreparationForge,
            &[ReleaseRole::PreparationAnonymous.job_id()],
            Some(gate),
            Some(ENV),
            fixture::preparation(bootstrap_mode)?,
        )?,
    );
    Ok(())
}

pub(crate) fn bootstrap_tools()
-> velnor_actions_workflow_renderer::release_bootstrap::ReleaseBootstrapApproval {
    bootstrap_fixture::bootstrap_tools(checkout_uses())
}

pub(crate) fn gated_spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    workflow(false, true)
}

pub(crate) fn anonymous_spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    workflow(false, false)
}

pub(crate) fn bootstrap_spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    workflow(true, true)
}

pub(crate) fn job_steps(spec: &ReleaseWorkflowSpec, id: &str) -> Option<Vec<Step>> {
    spec.jobs.get(id).map(|job| job.steps.clone())
}

pub(crate) fn with_steps(
    mut spec: ReleaseWorkflowSpec,
    id: &str,
    steps: Vec<Step>,
) -> Option<ReleaseWorkflowSpec> {
    spec.jobs.get_mut(id).map(|job| {
        job.steps = steps;
        spec
    })
}

#[test]
fn release_gate_rejects_mutated_typed_output_owner() -> Result<(), RenderError> {
    let mut workflow = gated_spec()?;
    let job = workflow
        .jobs
        .get_mut(ReleaseRole::RegistryPublishOidc.job_id())
        .expect("registry job");
    job.outputs[0].value.step_id = StepId::new("release-forge-receipt-artifact")?;
    assert!(invalid(check_release_jobs(&workflow)).is_some());
    Ok(())
}
