use super::*;

use velnor_actions_orchestrator_merge_ports::CoverBaselinePort;

/// Lookup stub that must never fire: these tests supply a manifest or
/// short-circuit before the live lookup.
struct NoLookup;

impl CoverBaselinePort for NoLookup {
    fn resolve_manifests(
        &self,
        _catalog: &velnor_actions_mise::ToolCatalog,
        _root: &std::path::Path,
        _base: &str,
        _workflow: &str,
        _branch: &str,
        _artifact: Option<&str>,
        _repository: Option<&str>,
    ) -> Result<Vec<BaselineManifest>, String> {
        unreachable!("test supplies manifest or short-circuits")
    }
}

mod cover_baseline_lookup_tests;
mod cover_baseline_tests;
