//! Group identities: canonical graph, bundles, lanes, toolchains (P03).
//! Declared via `#[path]` from `internal_plan.rs` (no `lib.rs` edit).

use serde::{Deserialize, Serialize};
use velnor_actions_contract::cachekey::{
    FormatInputs, LaneInputs, ToolchainInputs, cache_format_id, lane_id, toolchain_id,
};
use velnor_actions_contract::{ContractError, digest_b3};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::{DepKind, TaskGroup, WorkspaceRecord};

use super::snapshot::{
    ExecutionSnapshot, canonical_digest, normalized_component_id, platform_id_for,
};
use crate::discover::Discovery;
use crate::toolcheck::ToolInputCheck;

/// Cache-format label for the single Velnor cache payload version.
const CACHE_FORMAT_LABEL: &str = "velnor-cache-v1";

/// Canonical package/workspace graph over normalized manifests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotGraph {
    /// Normalized member component identities, sorted.
    pub(crate) members: Vec<String>,
    /// Local-path edges over normalized endpoints, sorted.
    pub(crate) edges: Vec<SnapshotEdge>,
}

/// One normalized local-path dependency edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotEdge {
    /// Normalized dependent component.
    pub(crate) from: String,
    /// Normalized path-dependency component.
    pub(crate) to: String,
    /// Dependency kind.
    pub(crate) kind: String,
    /// Whether the edge is optional.
    pub(crate) optional: bool,
    /// Target filter, `-` when absent.
    pub(crate) target: String,
}

/// Canonical graph over normalized manifests, never raw Cargo IDs.
pub(crate) fn snapshot_graph_for(record: &WorkspaceRecord) -> SnapshotGraph {
    let manifest_of = |id: &str| {
        record
            .packages
            .iter()
            .find(|package| package.id == id)
            .map_or_else(
                || normalized_component_id(id, ""),
                |package| normalized_component_id(id, &package.manifest),
            )
    };
    let mut members: Vec<String> = record.members.iter().map(|id| manifest_of(id)).collect();
    members.sort();
    members.dedup();
    let mut edges: Vec<SnapshotEdge> = record
        .edges
        .iter()
        .map(|edge| SnapshotEdge {
            from: manifest_of(&edge.from),
            to: manifest_of(&edge.to),
            kind: match edge.kind {
                DepKind::Normal => "normal",
                DepKind::Build => "build",
                DepKind::Dev => "dev",
            }
            .to_owned(),
            optional: edge.optional,
            target: edge.target.clone().unwrap_or_else(|| "-".to_owned()),
        })
        .collect();
    edges.sort_by(|left, right| edge_key(left).cmp(&edge_key(right)));
    SnapshotGraph { members, edges }
}

/// Sort key for one normalized dependency edge.
fn edge_key(edge: &SnapshotEdge) -> (&String, &String, &String, &String) {
    (&edge.from, &edge.to, &edge.kind, &edge.target)
}

/// Config digest over `(path, digest)` tool-input pairs.
///
/// Consumed tool files are semantic inputs; parse values, finding codes,
/// and advisory text stay out.
pub(crate) fn tool_config_digest(checks: &[ToolInputCheck]) -> String {
    let mut pairs: Vec<(&str, &str)> = checks
        .iter()
        .filter_map(|check| {
            check
                .digest
                .as_deref()
                .map(|digest| (check.path.as_str(), digest))
        })
        .collect();
    pairs.sort_unstable();
    canonical_digest(&pairs).unwrap_or_else(|_| digest_b3(b"config_error"))
}

/// Owned identity-extension inputs for one group (PAR-3.4, PAR-4.8).
#[derive(Debug, Clone)]
pub(crate) struct ExtensionBundle {
    /// Cargo package ID.
    package_id: String,
    /// Workspace identity digest.
    workspace_id: String,
    /// Execution profile name.
    profile: String,
    /// Normalized manifest path.
    manifest: String,
    /// Workspace graph digest.
    graph_digest: String,
    /// Target `kind:name` entries.
    targets: Vec<String>,
    /// Tool-input config digest.
    config_digest: String,
    /// Whether the package carries a build script.
    has_build_script: bool,
    /// Lockfile content digest, when resolved against the checkout.
    lock_digest: Option<String>,
    /// Nextest-config content digest, when resolved.
    nextest_digest: Option<String>,
    /// Declared rerun inputs; `Some([])` proves no build script reads.
    rerun_inputs: Option<Vec<String>>,
}

impl ExtensionBundle {
    /// Borrow the bundle as adapter inputs with resolved digests bound.
    pub(crate) fn inputs(&self) -> velnor_actions_rust::GroupExtensionInputs<'_> {
        velnor_actions_rust::GroupExtensionInputs {
            package_id: &self.package_id,
            workspace_id: &self.workspace_id,
            profile: &self.profile,
            manifest: &self.manifest,
            graph_digest: &self.graph_digest,
            targets: &self.targets,
            config_digest: &self.config_digest,
            lock_digest: self.lock_digest.as_deref(),
            nextest_digest: self.nextest_digest.as_deref(),
            archive_source: None,
            rerun_inputs: self.rerun_inputs.as_deref(),
            has_build_script: self.has_build_script,
        }
    }

    /// Tool-input config digest backing the bundle.
    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }

    /// Workspace graph digest backing the bundle.
    pub(crate) fn graph_digest(&self) -> &str {
        &self.graph_digest
    }
}

/// Snapshot-indexed bundle from a prebuilt [`ExecutionSnapshot`].
///
/// Graph and workspace digests come from the snapshot's once-built
/// index, never a per-group workspace rescan: every plan and coverage
/// path threads the one snapshot built per analysis. Package targets
/// and build-script facts still resolve from discovery. Lock and
/// Nextest digests bind checkout content when `root` is set.
pub(crate) fn extension_bundle_with_snapshot(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    group: &TaskGroup,
    root: Option<&std::path::Path>,
    nextest_config: Option<&str>,
) -> ExtensionBundle {
    let manifest = super::manifest_for_key(&group.manifest_key);
    let workspace_id = snapshot.workspace_id_for(&group.package_id, &manifest);
    let graph_digest = snapshot.graph_digest_for(&group.package_id, &manifest);
    let mut bundle = ExtensionBundle {
        package_id: group.package_id.clone(),
        workspace_id,
        profile: group.configuration.clone(),
        manifest,
        graph_digest,
        targets: Vec::new(),
        config_digest: tool_config_digest(&discovery.tool_checks),
        has_build_script: false,
        lock_digest: None,
        nextest_digest: None,
        rerun_inputs: None,
    };
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            let owned = package.id == group.package_id
                || (group.package_id.is_empty() && package.manifest == bundle.manifest);
            if !owned {
                continue;
            }
            bundle.targets = package
                .targets
                .iter()
                .map(|target| format!("{}:{}", target.kind, target.name))
                .collect();
            bundle.has_build_script = package.has_build_script;
        }
    }
    if !bundle.has_build_script {
        bundle.rerun_inputs = Some(Vec::new());
    }
    if let Some(root) = root {
        bundle.lock_digest = super::closure::lock_digest_at_root(root, &bundle.manifest);
        bundle.nextest_digest = super::closure::nextest_digest_at_root(root, nextest_config);
    }
    bundle
}

/// Platform identity for one group via [`platform_id_for`].
pub(crate) fn platform_id_for_group(label: &str, group: &TaskGroup) -> String {
    let target = if group.target == "host" {
        velnor_actions_contract::target_for_runner_label(label).unwrap_or("host")
    } else {
        group.target.as_str()
    };
    platform_id_for(label, target)
}

/// Writer lane from responsibility: shard suffix when sharded, else primary.
///
/// Ordinals never enter the identity: the same responsibility in any
/// schedule position yields the same lane, and distinct responsibilities
/// (including sibling shards) never share one.
pub(crate) fn writer_lane_for(task_id: &str) -> String {
    if let Some((_, shard)) = task_id.split_once("/shard-")
        && !shard.is_empty()
        && !shard.contains('/')
    {
        return format!("shard-{shard}");
    }
    "primary".to_owned()
}

/// Configuration digest for lane identity: config plus target and flags.
fn lane_config_digest(group: &TaskGroup) -> String {
    let mut features = group.features.clone();
    features.sort();
    let mut flags = group.target_flags.clone();
    flags.sort();
    canonical_digest(&serde_json::json!({
        "configuration": group.configuration,
        "target": group.target,
        "features": features,
        "flags": flags,
        "driver": group.compile_driver,
        "runner": group.test_runner,
    }))
    .unwrap_or_else(|_| digest_b3(b"lane_config_error"))
}

/// Lane identity inputs for one group: responsibility and config.
pub(crate) fn lane_inputs_for(group: &TaskGroup, workspace_id: &str) -> LaneInputs {
    LaneInputs {
        workspace_id: workspace_id.to_owned(),
        component_id: normalized_component_id(
            &group.package_id,
            &super::manifest_for_key(&group.manifest_key),
        ),
        task_kind: group.kind.as_str().to_owned(),
        configuration: lane_config_digest(group),
        writer_lane: writer_lane_for(&group.task_id),
    }
}

/// Lane identity digest, total over any group.
///
/// The centralized [`lane_id`] validates first; hostile inputs that fail
/// validation fall back to a plain digest over the same fields so two
/// distinct responsibilities still never collide.
pub(crate) fn lane_id_for(group: &TaskGroup, workspace_id: &str) -> String {
    let inputs = lane_inputs_for(group, workspace_id);
    if let Ok(id) = lane_id(&inputs) {
        return id;
    }
    canonical_digest(&inputs).unwrap_or_else(|_| digest_b3(b"lane_error"))
}

/// Toolchain inputs with exact component evidence, never `unreported`.
pub(crate) fn toolchain_inputs_for(group: &TaskGroup, catalog: &ToolCatalog) -> ToolchainInputs {
    let mut tools = vec![PinnedTool::Rust];
    if group.compile_driver == "mbx" {
        tools.push(PinnedTool::MrBoxington);
    }
    if group.test_runner == "cargo_nextest" {
        tools.push(PinnedTool::Nextest);
    }
    let mut specs = catalog.tool_specs(&tools);
    specs.sort();
    ToolchainInputs {
        tools: specs,
        components: velnor_actions_mise::PrepareRustComponents::components(),
        compile_driver: group.compile_driver.clone(),
        test_runner: group.test_runner.clone(),
    }
}

/// Toolchain identity digest over the centralized toolchain inputs.
///
/// # Errors
///
/// Returns [`ContractError`] for invalid toolchain inputs.
pub(crate) fn toolchain_digest_for(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    toolchain_id(&toolchain_inputs_for(group, catalog))
}

/// Cache-format identity for one compile driver.
///
/// The single cache format is versioned here; unknown drivers fall back
/// to an explicit driver-labeled digest instead of guessing a format.
pub(crate) fn cache_format_id_for(driver: &str) -> String {
    if (driver == "cargo" || driver == "mbx")
        && let Ok(id) = cache_format_id(&FormatInputs {
            adapter: driver.to_owned(),
            format: CACHE_FORMAT_LABEL.to_owned(),
            generation: "1".to_owned(),
        })
    {
        return id;
    }
    canonical_digest(&serde_json::json!({
        "schema": "velnor-cache-format-fallback-v1",
        "adapter": driver,
    }))
    .unwrap_or_else(|_| digest_b3(b"cache_format_error"))
}
