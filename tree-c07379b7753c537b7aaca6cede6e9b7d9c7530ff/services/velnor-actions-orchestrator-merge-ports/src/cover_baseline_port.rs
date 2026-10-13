//! Baseline manifest resolution consumed by cover-baseline, behind a port trait.
//!
//! Cover-baseline looks up live exact-base manifests through the hub's
//! sharded `gh` lookup. The hub implements [`CoverBaselinePort`] by
//! delegating to its cover modules, so the extracted cover-baseline
//! crate depends only on this contract and the direct
//! cover-baseline/cover cycle is broken.

use std::path::Path;

use velnor_actions_mise::ToolCatalog;

use super::merge_types::BaselineManifest;

/// Live exact-base manifest resolution for baseline classification.
pub trait CoverBaselinePort {
    /// Resolve manifests for one base through the sharded lookup.
    ///
    /// Mirrors `cover::shard::resolve_manifests` exactly: pins the
    /// catalog, checkout, base, workflow, branch, exact artifact name,
    /// and repository scope, returning every matching manifest or the
    /// lookup miss reason.
    fn resolve_manifests(
        &self,
        catalog: &ToolCatalog,
        root: &Path,
        base: &str,
        workflow: &str,
        branch: &str,
        artifact: Option<&str>,
        repository: Option<&str>,
    ) -> Result<Vec<BaselineManifest>, String>;
}
