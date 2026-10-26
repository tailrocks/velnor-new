// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, expand_workflow};
use velnor_actions_workflow_renderer::finalize_jobs as finalize_render_jobs;

use crate::OrchestratorError;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;

pub(crate) fn validate_published_generator_lock(
    lock: &velnor_actions_contract::GeneratorLock,
) -> Result<(), OrchestratorError> {
    lock.validate(".velnor/generator.lock")
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    lock.check_supported_targets(".velnor/generator.lock")
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    velnor_actions_mise::catalog::lock::verify_lock_against_published_release(lock).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

pub fn finalized_jobs(
    prep: &GenerationPreparation,
) -> Result<BTreeMap<String, Job>, OrchestratorError> {
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let ir = expand_workflow(&prep.workflow.ir, &prep.config, None).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(finalize_render_jobs(
        &ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
        &mise,
    )?)
}
