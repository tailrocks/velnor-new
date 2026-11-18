// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::Job;
use velnor_actions_contract::workflow::fork_fenced_condition;

use crate::RenderError;
use crate::render::RenderContext;

/// Moves the renderer-added control jobs that `execution.overrides` places on
/// the scale set, and applies the same fork fence the contract gives IR
/// control jobs.
pub(crate) fn apply(
    jobs: &mut BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    if let Some(gate) = jobs.get_mut(velnor_actions_contract::REQUIRED_GATE_JOB_ID) {
        ctx.runs_on.clone_into(&mut gate.runs_on);
    }
    if ctx.scale_set_support_jobs.is_empty() {
        return Ok(());
    }
    let selector = ctx.scale_set_selector.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow("support_scale_set_without_selector".to_owned())
    })?;
    for id in &ctx.scale_set_support_jobs {
        if id == velnor_actions_contract::REQUIRED_GATE_JOB_ID && jobs.contains_key(id) {
            crate::required_gate::add_fork_gate(jobs, &ctx.runs_on)?;
        }
        let Some(job) = jobs.get_mut(id) else {
            continue;
        };
        selector.token().clone_into(&mut job.runs_on);
        job.condition = Some(fork_fenced_condition(job.condition.as_deref()));
    }
    Ok(())
}
