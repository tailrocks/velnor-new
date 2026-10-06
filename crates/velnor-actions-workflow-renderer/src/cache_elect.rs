//! Workload jobs consume owned tool snapshots without exporting their state.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepKind};

use crate::{MiseSetup, RenderError};

/// Reject tool snapshot writes from jobs without admitted pure production.
///
/// Source-producer metadata grants no tool-cache authority. Their isolated
/// native source payloads do not overlap the owned executable installations.
///
/// # Errors
/// Returns [`RenderError`] for a cache writer that covers an owned tool root.
pub fn validate_tool_consumers(
    jobs: &BTreeMap<String, Job>,
    setup: &MiseSetup,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        if crate::cache_p08::tool_roles::validate_tool_producer(job, setup, records)? {
            continue;
        }
        if job.steps.iter().any(|step| {
            crate::cache_tool_paths::owned_transport(step)
                && matches!(&step.kind, StepKind::Action { uses, .. }
                    if uses.starts_with("actions/cache/save@")
                        || uses.starts_with("actions/cache@"))
        }) {
            return Err(RenderError::InvalidWorkflow(format!(
                "tool_consumer_write:{id}"
            )));
        }
    }
    Ok(())
}
