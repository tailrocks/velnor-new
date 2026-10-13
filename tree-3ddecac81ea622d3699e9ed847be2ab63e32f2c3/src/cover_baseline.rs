//! Baseline classification over the extracted cover-baseline crate.
//!
//! Evidence resolution and coverage classification live in
//! `velnor-actions-orchestrator-cover-baseline` and resolve live
//! manifests only through [`CoverBaselinePort`]; this module implements
//! the port over the hub's cover modules and keeps the original
//! `apply_baseline` entrypoint byte-identical for the plan caller.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract_workflow::{Plan, WorkflowEvent};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_cover_baseline::cover_baseline::apply_baseline_with;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_merge_ports::{BaselineManifest, CoverBaselinePort};

pub(crate) use velnor_actions_orchestrator_cover_baseline::cover_baseline::BaselineInputs;

/// Hub shard resolution behind the cover-baseline port.
struct HubBaseline;

impl CoverBaselinePort for HubBaseline {
    fn resolve_manifests(
        &self,
        catalog: &ToolCatalog,
        root: &Path,
        base: &str,
        workflow: &str,
        branch: &str,
        artifact: Option<&str>,
        repository: Option<&str>,
    ) -> Result<Vec<BaselineManifest>, String> {
        crate::cover::shard::resolve_manifests(
            catalog, root, base, workflow, branch, artifact, repository,
        )
    }
}

/// Classify obligations against baseline evidence, or execute everything.
///
/// A provided manifest is validated and applied; otherwise a live exact-base
/// lookup runs when the plan has a base and obligations. Every miss keeps
/// full execution and records its exact reason: cache misses, corruption,
/// and expiry broaden instead of failing, while coverage that would
/// invalidate the plan reverts to full execution with a warning.
/// Validation runs fresh on every call; reports never replay cached
/// verdicts.
/// # Errors
pub(crate) fn apply_baseline(
    plan: &mut Plan,
    event: WorkflowEvent,
    inputs: BaselineInputs<'_>,
    manifest: Option<BaselineManifest>,
    discovery: &Discovery,
    changed: Option<&BTreeSet<String>>,
) -> Result<(), OrchestratorError> {
    apply_baseline_with(
        &HubBaseline,
        plan,
        event,
        inputs,
        manifest,
        discovery,
        changed,
    )
}
