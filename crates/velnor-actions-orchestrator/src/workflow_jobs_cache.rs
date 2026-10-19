//! Plan-job cache steps: runtime keys, sources restore, producer-gated save.
//!
//! Split from [`crate::workflow_jobs`] (size gate): this module owns how the
//! plan job computes cache keys, primes the Cargo-source cache before
//! fetch, and publishes it after fetch.

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, key_step_expr, rsdeps_key_output,
};
use velnor_actions_contract::{Step, StepId};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::steps::{SeedCacheUse, seed_cache_step};

use crate::OrchestratorError;
use crate::cache_keys::CacheKeyInputs;

/// Exact-key probes of every seeded root; outputs are the expected tiers.
fn seed_lookups(roots: &[String]) -> Result<Vec<Step>, OrchestratorError> {
    let uses = crate::cache_seed::cache_uses(true)?;
    roots
        .iter()
        .enumerate()
        .map(|(ordinal, root)| {
            let mut step = seed_cache_step(
                &uses,
                CacheFamily::Rsdeps,
                root,
                SeedCacheUse::Lookup,
                &key_step_expr(&rsdeps_key_output(ordinal)),
                None,
            )?;
            step.id = Some(StepId::rsdeps_lookup(ordinal).ok_or_else(|| {
                OrchestratorError::Contract {
                    problem: "too_many_seed_roots".to_owned(),
                }
            })?);
            Ok(step)
        })
        .collect()
}

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
/// Returns contract, actionlint, or render errors for bad roots or pins.
pub(crate) fn cache_steps_for_plan(
    catalog: &ToolCatalog,
    keys: &CacheKeyInputs,
) -> Result<PlanCache, OrchestratorError> {
    let mut restore = Vec::new();
    let key_step = crate::cache_keys::cache_keys_step(catalog, keys)?;
    let has_key_step = key_step.is_some();
    restore.extend(key_step);
    if has_key_step {
        restore.extend(seed_lookups(&keys.seed.roots)?);
    }
    let mut save = Vec::new();
    if !keys.fetch_roots.is_empty() {
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
