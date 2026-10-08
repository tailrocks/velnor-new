//! One typed job template expanded into provider-specific artifact matrices.

use std::collections::BTreeMap;

use velnor_actions_contract_config::VerificationRunner;
use velnor_actions_contract_workflow::{
    ARTIFACT_MATRIX_MAX_PARALLEL_ENV, ARTIFACT_MATRIX_NEEDS_JOB_ENV, ARTIFACT_MATRIX_PROVIDER_ENV,
    Job, JobTimeout, PermissionLevel, Permissions, Step,
};
use velnor_actions_workflow_jobs::{context::PLAN_JOB_ID, download_plan_step};
use velnor_actions_workflow_steps::{MiseSetup, mise_setup_step, shell_step, steps};

/// Logical job ID before provider-lane expansion.
pub(crate) const ARTIFACT_BUILD_JOB_ID: &str = "artifact-build";
/// Matrix environment value resolved from the plan entry.
const ARTIFACT_MISE_TASK_ENV: &str = "VELNOR_ARTIFACT_MISE_TASK";
/// Build one Linux/x64 artifact task job for normal schema-2 routing.
///
/// The task and provider values are bound by the typed plan output and the
/// strict renderer's two provider-specific matrix attachments.
/// # Errors
pub(crate) fn build(
    mise_setup: &MiseSetup,
    checkout_uses: &str,
    max_parallel: u32,
    acquire: Option<Step>,
) -> Result<Job, velnor_actions_workflow_steps::RenderError> {
    let run_env = BTreeMap::from([
        (
            ARTIFACT_MISE_TASK_ENV.to_owned(),
            "${{ matrix.mise_task }}".to_owned(),
        ),
        (
            ARTIFACT_MATRIX_NEEDS_JOB_ENV.to_owned(),
            PLAN_JOB_ID.to_owned(),
        ),
        (
            ARTIFACT_MATRIX_PROVIDER_ENV.to_owned(),
            "provider".to_owned(),
        ),
        (
            ARTIFACT_MATRIX_MAX_PARALLEL_ENV.to_owned(),
            max_parallel.to_string(),
        ),
    ]);
    // The task command uses a fixed env key; marker env is consumed only by
    // the renderer and stripped before validation/emission.
    let task = shell_step(
        "Run declared artifact task",
        vec![
            "mise".to_owned(),
            "run".to_owned(),
            format!("${ARTIFACT_MISE_TASK_ENV}"),
        ],
        run_env,
    )?;
    let mut steps = vec![steps::checkout_step(checkout_uses)?];
    steps.extend(acquire);
    steps.push(download_plan_step()?);
    steps.push(mise_setup_step(mise_setup)?);
    steps.push(shell_step(
        "Install locked artifact task tools",
        vec![
            "mise".to_owned(),
            "install".to_owned(),
            "--locked".to_owned(),
        ],
        BTreeMap::new(),
    )?);
    steps.push(task);
    steps.push(velnor_actions_workflow_steps::steps::artifact_export_step());
    steps.push(velnor_actions_workflow_steps::steps::artifact_build_upload_step()?);
    Ok(Job {
        display_name: "Build declared artifact".to_owned(),
        runs_on: VerificationRunner::LinuxX64.runs_on().to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::None,
        }),
        environment: None,
        steps,
    })
}
/// Build the artifact job from validated config and the pinned runner setup.
/// # Errors
pub(crate) fn from_config(
    config: &velnor_actions_contract_config::VelnorConfig,
    checkout_uses: &str,
    acquire: Option<Step>,
) -> Result<Option<Job>, velnor_actions_orchestrator_core::OrchestratorError> {
    if config.workflow.artifact_tasks.is_empty() {
        return Ok(None);
    }
    let setup = velnor_actions_orchestrator_pins::pins::resolve_verification_mise_setup(
        config,
        VerificationRunner::LinuxX64,
    )?;
    let job = build(
        &setup,
        checkout_uses,
        config.workflow.max_parallel_jobs,
        acquire,
    )
    .map_err(
        |error| velnor_actions_orchestrator_core::OrchestratorError::Contract {
            problem: error.to_string(),
        },
    )?;
    Ok(Some(job))
}
