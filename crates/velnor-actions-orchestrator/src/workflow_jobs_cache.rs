//! Plan-job cache steps: trusted-writer restore/save pair.
//!
//! Split from [`crate::workflow_jobs`] (size gate): this module owns how the
//! plan job primes and publishes the Cargo-source cache before/after fetch.

use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;

/// Plan-job cache steps: restore before fetch, save after (writer only).
///
/// Lockless emits nothing. MBX repos restore/save the shared `actions/cache`
/// snapshot; Cargo-only repos emit one `rust-cache` writer step (its post
/// action saves; no separate save step).
pub(crate) struct PlanCache {
    /// Restore steps (before fetch).
    pub(crate) restore: Vec<Step>,
    /// Save steps (after fetch, writer only).
    pub(crate) save: Vec<Step>,
}

/// Cache steps for the plan job's trusted-writer role.
/// # Errors
///
/// Returns contract, actionlint, or render errors for bad labels or pins.
pub(crate) fn cache_steps_for_plan(
    label: &str,
    catalog: &ToolCatalog,
    use_mbx: bool,
    fetch_roots: &[String],
) -> Result<PlanCache, OrchestratorError> {
    if fetch_roots.is_empty() {
        return Ok(PlanCache {
            restore: Vec::new(),
            save: Vec::new(),
        });
    }
    let target = velnor_actions_contract::target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        }
    })?;
    let rust = catalog.version(PinnedTool::Rust);
    if use_mbx {
        let key = crate::source_cache::sources_cache_key(target, rust, fetch_roots)?;
        let prefix = crate::source_cache::sources_restore_prefix(&key);
        let restore = crate::source_cache::sources_restore_step(&key, &[prefix])?;
        let save = crate::source_cache::sources_save_step(&key)?;
        return Ok(PlanCache {
            restore: vec![restore],
            save: vec![save],
        });
    }
    let shared = format!(
        "{}-{target}-{rust}",
        crate::source_cache::RUST_CACHE_SHARED_PREFIX
    );
    let writer = crate::source_cache::rust_cache_step(&shared, true)?;
    Ok(PlanCache {
        restore: vec![writer],
        save: Vec::new(),
    })
}
