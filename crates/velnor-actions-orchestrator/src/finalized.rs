//! Finalized jobs: the attached, merged, closed IR `generate` writes.
//!
//! [`owned_preparation`] applies the Velnor lock/preseed attach;
//! [`finalized_jobs`] runs the full renderer finalization over it. Both
//! `generate` and [`crate::plan_text_checked`] build from these so the two
//! surfaces agree by construction instead of echoing different
//! pipeline stages.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::finalize_jobs as finalize_render_jobs;

use crate::OrchestratorError;
use crate::attach::{attach_lock_acquire, attach_preseed, requalify_source_helpers};
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
        match verify_velnor_repository_files(&prep.root)? {
            Some(lock) => attach_lock_acquire(
                &mut owned.workflow.ir,
                &lock,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?,
            None => attach_preseed(
                &mut owned.workflow,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?,
        }
    }
    requalify_source_helpers(&mut owned.workflow, env!("CARGO_PKG_VERSION"))?;
    finalize_helper_domain(&mut owned)?;
    Ok(owned)
}

/// Capture the actual attached helper source computation without runtime admission.
fn finalize_helper_domain(owned: &mut GenerationPreparation) -> Result<(), OrchestratorError> {
    if owned.config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(());
    }
    let version = env!("CARGO_PKG_VERSION");
    let setup = resolve_mise_setup(&owned.config, &owned.runner_label)?;
    let result = crate::workflow_mbx_finalize::finalize_helper(
        &owned.workflow,
        &velnor_actions_mise::ToolCatalog::pinned(),
        &setup,
        version,
    )?;
    let helper = velnor_actions_contract::MbxCacheDomain::Helper;
    let sidecar = &mut owned.workflow.mbx_finalization;
    sidecar
        .domains
        .retain(|domain| domain.descriptor.domain != helper);
    sidecar.unsupported.retain(|domain| domain.domain != helper);
    for record in &result.source_helpers {
        if !owned.workflow.context.source_helpers.contains(record) {
            owned.workflow.context.source_helpers.push(record.clone());
        }
        if !sidecar.source_helpers.contains(record) {
            sidecar.source_helpers.push(record.clone());
        }
    }
    for draft in &result.receipt_drafts {
        for drafts in [
            &mut owned.workflow.receipt_drafts,
            &mut sidecar.receipt_drafts,
        ] {
            if drafts.iter().any(|previous| {
                previous.file.path == draft.file.path && previous.file != draft.file
            }) {
                return Err(OrchestratorError::Contract {
                    problem: format!("mbx_helper_receipt_source_conflict:{}", draft.file.path),
                });
            }
            if !drafts.iter().any(|previous| previous.file == draft.file) {
                drafts.push(draft.clone());
            }
        }
    }
    owned.discovery.recommendations.extend(
        result
            .unsupported
            .iter()
            .map(|domain| format!("mbx_cache_cold:{}:{}", domain.job_id, domain.reason)),
    );
    owned.discovery.recommendations.sort();
    owned.discovery.recommendations.dedup();
    sidecar.domains.extend(result.domains);
    sidecar.unsupported.extend(result.unsupported);
    Ok(())
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
    jobs_for_owned_preparation(&owned)
}

/// Render jobs from the same already attached preparation used by reports.
pub(crate) fn jobs_for_owned_preparation(
    owned: &GenerationPreparation,
) -> Result<BTreeMap<String, Job>, OrchestratorError> {
    let mise = resolve_mise_setup(&owned.config, &owned.runner_label)?;
    Ok(finalize_render_jobs(
        &owned.workflow.ir,
        owned.config.workflow.policy,
        owned.workflow.support.as_ref(),
        &owned.workflow.context,
        &mise,
    )?)
}
