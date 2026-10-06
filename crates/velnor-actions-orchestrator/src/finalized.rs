//! Finalized jobs: the attached, merged, closed IR `generate` writes.
//!
//! [`owned_preparation`] applies the Velnor lock/preseed attach;
//! [`finalized_jobs`] runs the full renderer finalization over it. Both
//! `generate` and [`crate::plan_text`] build from these so the two
//! surfaces agree by construction instead of echoing different
//! pipeline stages.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy, expand_workflow};
use velnor_actions_workflow_renderer::finalize_jobs as finalize_render_jobs;

use crate::OrchestratorError;
use crate::attach::{attach_lock_acquire, attach_preseed};
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::validate::verify_velnor_repository_files;

/// Owned preparation with the Velnor lock/preseed attach applied.
///
/// Shared by [`crate::render_staged_tree`] and [`finalized_jobs`] so
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
        if let Some(lock) = verify_velnor_repository_files(&prep.root)? {
            attach_lock_acquire(
                &mut owned.workflow.ir,
                &lock,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?;
        } else {
            attach_preseed(
                &mut owned.workflow,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?;
        }
    }
    Ok(owned)
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
