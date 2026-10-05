//! Read-only restore selection for crate-job inputs.

use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;

/// Restore step for one crate: shared sources, or Cargo-only registry.
///
/// Lockless emits nothing, and tofu roles restore providers through
/// the separate provider-cache step (never here). Cargo-only repos
/// (no MBX anywhere) restore via pinned `rust-cache` (read-only);
/// every other lockful crate restores the shared `actions/cache`
/// snapshot (read-only, never saves the shared key).
pub(super) fn restore_step_for_crate(
    label: &str,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
    use_mbx: bool,
    repo_has_mbx: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if !use_rust || fetch_roots.is_empty() {
        return Ok(None);
    }
    let target = velnor_actions_contract::target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        }
    })?;
    let rust = catalog.version(PinnedTool::Rust);
    if !use_mbx && !repo_has_mbx {
        let shared = format!(
            "{}-{target}-{rust}",
            crate::source_cache::RUST_CACHE_SHARED_PREFIX
        );
        return crate::source_cache::rust_cache_step(&shared, false).map(Some);
    }
    let key = crate::source_cache::sources_cache_key(target, rust, fetch_roots)?;
    let prefix = crate::source_cache::sources_restore_prefix(&key);
    crate::source_cache::sources_restore_step(&key, &[prefix]).map(Some)
}
