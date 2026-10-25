use std::collections::BTreeMap;

use velnor_actions_contract::{Job, WorkflowPolicy, expand_workflow};
use velnor_actions_workflow_renderer::finalize_jobs as finalize_render_jobs;

use crate::OrchestratorError;
use crate::attach::{attach_lock_acquire, attach_preseed};
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::validate::verify_velnor_repository_files;

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
                &prep.emission_catalog(),
            )?;
        }
    }
    Ok(owned)
}

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
