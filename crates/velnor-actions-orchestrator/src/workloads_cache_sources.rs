//! Immutable native source candidates cross task grouping without public admission.

use velnor_actions_contract::{ProposedTask, Stack};

use crate::OrchestratorError;
use crate::workloads::cache_eligibility::{NativeNpmSource, valid_source_candidate};

const NPM_KEY: &str = "VELNOR_NATIVE_NPM_SOURCE_CANDIDATES";
const BUN_KEY: &str = "VELNOR_NATIVE_BUN_SOURCE_CANDIDATES";
const MAX_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;
const MAX_SOURCES: usize = 1024;

/// Every grouped obligation must declare the same closed candidate set.
/// Absence disables optional transport; invalid generated metadata is an error.
/// A successful result is lock evidence only, never anonymous public proof.
pub(crate) fn native_sources_for_tasks(
    tasks: &[&ProposedTask],
) -> Result<Vec<NativeNpmSource>, OrchestratorError> {
    let Some(first) = tasks.first() else {
        return Ok(Vec::new());
    };
    if !tasks.iter().any(|task| has_marker(task)) {
        return Ok(Vec::new());
    }
    let (key, forbidden) = match first.configuration.as_str() {
        "node_ci" => (NPM_KEY, BUN_KEY),
        "bun_ci" => (BUN_KEY, NPM_KEY),
        _ => return Err(invalid("wrong_kind")),
    };
    let mut selected = None;
    for task in tasks {
        if task.stack_id != Stack::Workload.id()
            || task.configuration != first.configuration
            || task.identity.unit_id != first.identity.unit_id
            || task.identity.unit_path != first.identity.unit_path
            || task.identity.project_root != first.identity.project_root
            || task.identity.environment.contains_key(forbidden)
        {
            return Err(invalid("mixed_group"));
        }
        let descriptor = task
            .identity
            .environment
            .get(key)
            .ok_or_else(|| invalid("missing_member"))?;
        let sources = decode(descriptor)?;
        if selected
            .as_ref()
            .is_some_and(|previous| *previous != sources)
        {
            return Err(invalid("contradictory_members"));
        }
        selected = Some(sources);
    }
    Ok(selected.unwrap_or_default())
}

fn has_marker(task: &ProposedTask) -> bool {
    task.identity.environment.contains_key(NPM_KEY)
        || task.identity.environment.contains_key(BUN_KEY)
}

fn decode(descriptor: &str) -> Result<Vec<NativeNpmSource>, OrchestratorError> {
    if descriptor.len() > MAX_DESCRIPTOR_BYTES {
        return Err(invalid("descriptor_size"));
    }
    let mut sources: Vec<NativeNpmSource> =
        serde_json::from_str(descriptor).map_err(|_| invalid("malformed_descriptor"))?;
    if sources.len() > MAX_SOURCES || !sources.iter().all(valid_source_candidate) {
        return Err(invalid("invalid_candidate"));
    }
    sources.sort();
    sources.dedup();
    Ok(sources)
}

fn invalid(reason: &str) -> OrchestratorError {
    crate::internal::internal(&format!("native_source_metadata:{reason}"))
}

#[cfg(test)]
#[path = "workloads_cache_sources_tests.rs"]
mod tests;
