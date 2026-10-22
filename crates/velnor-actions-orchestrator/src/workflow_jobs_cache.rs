//! Plan-job cache steps: runtime keys, sources restore, producer-gated save.
//!
//! Split from [`crate::workflow_jobs`] (size gate): this module owns how the
//! plan job computes cache keys, primes the Cargo-source cache before
//! fetch, and publishes it after fetch.

use velnor_actions_contract::{Step, StepId};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

/// Plan-job cache steps around the fetch steps.
///
/// Lockless, tofu-free plans emit nothing. Every Rust repo uses the same
/// exact-path `actions/cache` snapshot, independent of compiler driver.
pub(crate) struct PlanCache {
    /// Key step and restore steps (before fetch).
    pub(crate) restore: Vec<Step>,
    /// Save steps (after fetch).
    pub(crate) save: Vec<Step>,
}

/// Cache steps for the plan job's single-writer role.
/// # Errors
///
/// Returns contract, workflow-lint, or render errors for bad roots or pins.
pub(crate) fn cache_steps_for_plan(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    tofu_roots: &[String],
) -> Result<PlanCache, OrchestratorError> {
    let mut restore = Vec::new();
    restore.extend(crate::cache_keys::cache_keys_step(
        catalog,
        fetch_roots,
        tofu_roots,
    )?);
    let mut save = Vec::new();
    if !fetch_roots.is_empty() {
        let (sources_restore, sources_save) = crate::source_cache::plan_sources_steps()?;
        restore.push(sources_restore);
        save.push(sources_save);
    }
    Ok(PlanCache { restore, save })
}

/// Give the last Plan fetch step the id the sources save gates on.
///
/// Fetch steps are unconditional and sequential, so the last one succeeds
/// only when every earlier one did.
pub(crate) fn mark_writer_gate(source_steps: &mut [Step]) {
    if let Some(last) = source_steps.last_mut() {
        last.id = Some(StepId::SourcesFetch);
    }
}
