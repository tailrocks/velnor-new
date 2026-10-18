//! Finalized jobs: the attached, merged, closed IR `generate` writes.
//!
//! [`owned_preparation`] applies the Velnor lock/preseed attach;
//! [`finalized_jobs`] runs the full renderer finalization over it. Both
//! `generate` and [`crate::plan_text`] build from these so the two
//! surfaces agree by construction instead of echoing different
//! pipeline stages.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy, expand_workflow_with_qualification_sources};
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
            let staged = format!(
                "{}{}",
                velnor_actions_workflow_renderer::steps::STAGED_BINARY_PREFIX,
                env!("CARGO_PKG_VERSION")
            );
            owned.workflow.context.verified_cache_helpers =
                locked_cache_helpers(&owned.workflow.ir.jobs, &lock, &prep.runner_label, &staged)?;
            owned.workflow.context.require_verified_cache_helper = true;
        } else {
            attach_preseed(
                &mut owned.workflow,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
                &prep.catalog,
            )?;
        }
    }
    Ok(owned)
}

fn locked_cache_helpers(
    jobs: &BTreeMap<String, Job>,
    lock: &velnor_actions_contract::GeneratorLock,
    default_label: &str,
    staged: &str,
) -> Result<BTreeMap<String, Vec<velnor_actions_contract::Step>>, OrchestratorError> {
    let mut helpers = BTreeMap::from([(
        default_label.to_owned(),
        vec![crate::pins::lock_acquire_step(lock, default_label, staged)?],
    )]);
    for (id, job) in jobs {
        let helper = if let Some(runner) = &job.check_runner {
            Some(crate::pins::lock_acquire_for_runner(lock, runner, staged)?)
        } else if velnor_actions_contract::is_crate_job_id(id) {
            Some(crate::pins::lock_acquire_step(lock, default_label, staged)?)
        } else if id.starts_with("suite-") {
            let label = crate::rust_suites_emit::helper_release_label(&job.runs_on);
            Some(crate::pins::lock_acquire_step(lock, &label, staged)?)
        } else {
            None
        };
        if let Some(helper) = helper {
            helpers
                .entry(job.runs_on.clone())
                .or_insert_with(|| vec![helper]);
        }
    }
    Ok(helpers)
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
    let qualification_sources = crate::crate_job_ids::qualification_job_sources_for_ir(
        &owned.discovery.proposals,
        &owned.workflow.ir,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let ir = expand_workflow_with_qualification_sources(
        &owned.workflow.ir,
        &owned.config,
        None,
        &qualification_sources,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    Ok(finalize_render_jobs(
        &ir,
        owned.config.workflow.policy,
        owned.workflow.support.as_ref(),
        &owned.workflow.context,
        &mise,
    )?)
}
