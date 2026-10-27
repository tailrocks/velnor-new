// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, expand_workflow_with_bun_plan};
use velnor_actions_workflow_renderer::finalize_jobs as finalize_render_jobs;

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;

pub(crate) fn validate_approved_generator_lock(
    lock: &velnor_actions_contract::GeneratorLock,
    approved: &crate::controller_approval::ControllerApprovedRelease,
) -> Result<(), OrchestratorError> {
    velnor_actions_workflow_renderer::release_helpers::validate_approved_generator_lock(
        lock,
        &approved.as_authority(),
    )
    .map_err(crate::local_release_manifest::map_release_helper_error)
}

pub fn finalized_jobs(
    prep: &GenerationPreparation,
) -> Result<BTreeMap<String, Job>, OrchestratorError> {
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let expansion =
        expand_workflow_with_bun_plan(&prep.workflow.ir, &prep.config, None).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
    let mut context = prep.workflow.context.clone();
    context.bun_lane_plan = expansion.bun_lane_plan;
    Ok(finalize_render_jobs(
        &expansion.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &context,
        &mise,
    )?)
}
