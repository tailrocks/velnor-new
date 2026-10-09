//! Finalized jobs: the attached, merged, closed IR `generate` writes.
//!
//! [`owned_preparation`] applies the Velnor lock/preseed attach;
//! [`finalized_jobs`] runs the full renderer finalization over it. Both
//! `generate` and `plan_text` build from these so the two
//! surfaces agree by construction instead of echoing different
//! pipeline stages.

use std::collections::BTreeMap;

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{Job, Step, StepRole, expand_workflow};
use velnor_actions_orchestrator_workflow_ir::workflow::WorkflowPlan;
use velnor_actions_workflow_jobs::finalize_jobs as finalize_render_jobs;

use crate::attach::{attach_lock_acquire, attach_preseed};
use crate::prepare::GenerationPreparation;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_pins::pins::resolve_mise_setup;

/// Owned preparation with the Velnor lock/preseed attach applied.
///
/// Shared by [`crate::generate::render_staged_tree`] and [`finalized_jobs`] so
/// `plan` lists the same finalized IR `generate` writes, never the
/// pre-attach discovery IR.
///
/// # Errors
///
/// Returns lock or attach errors from the Velnor-only file checks.
pub(crate) fn owned_preparation(
    prep: &GenerationPreparation,
) -> Result<GenerationPreparation, OrchestratorError> {
    let mut owned = prep.clone();
    if prep.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
        if let Some(lock) =
            velnor_actions_orchestrator_staged_validation::validate::verify_velnor_repository_files(
                &prep.root,
            )?
        {
            attach_lock_acquire(
                &mut owned.workflow.ir,
                &lock,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?;
        } else {
            let fetch_roots = velnor_actions_orchestrator_provisioning::source_prep::lockful_roots(
                &prep.root,
                &prep.discovery.workspaces,
            );
            attach_preseed(
                &mut owned.workflow,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
                &fetch_roots,
            )?;
        }
        bind_verification_staging(&mut owned.workflow)?;
    }
    Ok(owned)
}

/// Bind each output task to the exact helper-staging prefix attached above.
fn bind_verification_staging(workflow: &mut WorkflowPlan) -> Result<(), OrchestratorError> {
    for policy in &mut workflow.context.verification_tasks {
        if policy.task.outputs.is_empty() {
            continue;
        }
        let id = policy.job_id();
        let job = workflow
            .ir
            .jobs
            .get(&id)
            .ok_or_else(|| OrchestratorError::Contract {
                problem: format!("verification_job_missing:{id}"),
            })?;
        if job
            .steps
            .first()
            .is_none_or(|step| step.role != Some(StepRole::Checkout))
        {
            return Err(OrchestratorError::Contract {
                problem: format!("verification_checkout_missing:{id}"),
            });
        }
        let tail = &job.steps[1..];
        let count = tail
            .iter()
            .take_while(|step| is_task_staging_step(step))
            .count();
        if count == 0 || tail[count..].iter().any(is_task_staging_step) {
            return Err(OrchestratorError::Contract {
                problem: format!("verification_task_staging_missing_or_misordered:{id}"),
            });
        }
        policy.staging_steps = tail[..count].to_vec();
    }
    Ok(())
}

/// Staging roles supplied by Velnor's lock or pre-seed acquisition paths.
fn is_task_staging_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(
            StepRole::AcquireVelnor
                | StepRole::PreseedDownload
                | StepRole::PreseedVerifyManifest
                | StepRole::PreseedStage
        )
    )
}

/// Finalized jobs `generate` writes: attached IR plus merged support,
/// setup insertion, closures, and the final fan-in.
///
/// `plan` lists these jobs so its job table, step counts, and action
/// pins match the written YAML by construction.
///
/// # Errors
///
/// Returns lock, render, actionlint, or unsafe-path errors.
pub fn finalized_jobs(
    prep: &GenerationPreparation,
) -> Result<BTreeMap<String, Job>, OrchestratorError> {
    let owned = owned_preparation(prep)?;
    let mise = resolve_mise_setup(&owned.config, &owned.runner_label)?;
    let ir = expand_workflow(&owned.workflow.ir, &owned.config, None).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(finalize_render_jobs(
        &ir,
        owned.config.workflow.policy,
        owned.workflow.support.as_ref(),
        &owned.workflow.context,
        &mise,
    )?)
}
