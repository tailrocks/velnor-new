//! Task identities: canonical graph, bundles, lanes, toolchains (P03).
//! Declared via `#[path]` from `internal_plan.rs` (no `lib.rs` edit).

use serde::{Deserialize, Serialize};
use velnor_actions_contract::cachekey::{
    FormatInputs, LaneInputs, ToolchainInputs, cache_format_id, lane_id, mbx_cache_generation,
    toolchain_id,
};
use velnor_actions_contract::{ContractError, Stack, digest_b3};
use velnor_actions_contract_planning::{ProposedTask, component_id_for_unit};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::tool_needs;
use velnor_actions_rust_core::{CompileDriver, DepKind, WorkspaceRecord};

use super::snapshot::{ExecutionSnapshot, canonical_digest, platform_id_for};
use crate::discover::Discovery;
use crate::toolcheck::ToolInputCheck;

const CACHE_FORMAT_LABEL: &str = "velnor-cache-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotGraph {
    /// Normalized member component identities, sorted.
    pub(crate) members: Vec<String>,
    /// Local-path edges over normalized endpoints, sorted.
    pub(crate) edges: Vec<SnapshotEdge>,
}

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

pub(crate) fn snapshot_graph_for(record: &WorkspaceRecord) -> SnapshotGraph {
    let manifest_of = |id: &str| {
        record
            .packages
            .iter()
            .find(|package| package.id == id)
            .map_or_else(
                || component_id_for_unit(id, ""),
                |package| component_id_for_unit(id, &package.manifest),
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

fn edge_key(edge: &SnapshotEdge) -> (&String, &String, &String, &String) {
    (&edge.from, &edge.to, &edge.kind, &edge.target)
}

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
    /// Lockfile slot, resolved against the checkout.
    lock_digest: velnor_actions_rust::tasks::DigestSlot,
    /// Nextest-config slot, resolved against the checkout.
    nextest_digest: velnor_actions_rust::tasks::DigestSlot,
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
            lock_digest: self.lock_digest.clone(),
            nextest_digest: self.nextest_digest.clone(),
            archive_source: None,
            rerun_inputs: self.rerun_inputs.as_deref(),
            has_build_script: self.has_build_script,
        }
    }

    /// Tool-input config digest backing the bundle.
    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }

    /// Workspace identity digest backing the bundle.
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// Workspace graph digest backing the bundle.
    pub(crate) fn graph_digest(&self) -> &str {
        &self.graph_digest
    }
}

pub(crate) fn extension_bundle_with_snapshot(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    task: &ProposedTask,
    root: Option<&std::path::Path>,
    nextest_config: Option<&str>,
) -> ExtensionBundle {
    let manifest = task.identity.unit_path.clone();
    let workspace_id = snapshot.workspace_id_for(&task.identity.unit_id, &manifest);
    let graph_digest = snapshot.graph_digest_for(&task.identity.unit_id, &manifest);
    let mut bundle = ExtensionBundle {
        package_id: task.identity.unit_id.clone(),
        workspace_id,
        profile: task.configuration.clone(),
        manifest,
        graph_digest,
        targets: Vec::new(),
        config_digest: tool_config_digest(&discovery.tool_checks),
        has_build_script: false,
        lock_digest: velnor_actions_rust::tasks::DigestSlot::Unknown("no_checkout".to_owned()),
        nextest_digest: velnor_actions_rust::tasks::DigestSlot::Unknown("no_checkout".to_owned()),
        rerun_inputs: None,
    };
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            let owned = package.id == task.identity.unit_id
                || (task.identity.unit_id.is_empty() && package.manifest == bundle.manifest);
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
    if Stack::from_id(&task.stack_id) != Some(Stack::Rust) {
        // Tofu slots resolve through the tofu bridge (root lockfile);
        // the rust probes below would bind Cargo content instead.
        bundle.lock_digest =
            velnor_actions_rust::tasks::DigestSlot::Unknown("tofu_adapter_owned".to_owned());
        bundle.nextest_digest =
            velnor_actions_rust::tasks::DigestSlot::Unknown("tofu_adapter_owned".to_owned());
    } else if let Some(root) = root {
        bundle.lock_digest = super::closure_slots::lock_digest_at_root(root, &bundle.manifest);
        bundle.nextest_digest = super::closure_slots::nextest_digest_at_root(root, nextest_config);
    }
    bundle
}

/// # Errors
pub(crate) fn platform_id_for_group(
    label: &str,
    task: &ProposedTask,
) -> Result<String, ContractError> {
    let stack = Stack::require_known(&task.stack_id)?;
    if stack == Stack::Mise {
        let label = task.runner_profile.as_str();
        if let Some(host) = velnor_actions_contract_release::ReleaseTarget::for_runner_label(label)
            .map(velnor_actions_contract_release::ReleaseTarget::triple)
            && host != task.identity.target
        {
            return Err(ContractError::identity(
                "runner_label",
                "runner_target_mismatch",
            ));
        }
        return platform_id_for(label, &task.identity.target);
    }
    let host = velnor_actions_contract_release::ReleaseTarget::for_runner_label(label)
        .filter(|target| *target == velnor_actions_contract_release::ReleaseTarget::LinuxX86_64)
        .map(velnor_actions_contract_release::ReleaseTarget::triple)
        .ok_or_else(|| {
            ContractError::identity(
                "runner_label",
                format!("unsupported_target_for_runner:{label}"),
            )
        })?;
    let target = if task.identity.target == "host" {
        host
    } else {
        task.identity.target.as_str()
    };
    let identity = platform_id_for(label, target)?;
    if target != host {
        return Err(ContractError::identity("target", "runner_target_mismatch"));
    }
    Ok(identity)
}

pub(crate) fn writer_lane_for(task_id: &str) -> String {
    if let Some((_, index, count)) = velnor_actions_contract::split_shard_suffix(task_id) {
        return format!("shard-{index}-of-{count}");
    }
    "primary".to_owned()
}

fn lane_config_digest(task: &ProposedTask) -> String {
    let mut features = task.identity.features.clone();
    features.sort();
    let mut flags = task.identity.flags.clone();
    flags.sort();
    canonical_digest(&serde_json::json!({
        "configuration": task.configuration,
        "target": task.identity.target,
        "features": features,
        "flags": flags,
        "driver": task.identity.compile_driver,
        "runner": task.identity.test_runner,
    }))
    .unwrap_or_else(|_| digest_b3(b"lane_config_error"))
}

/// Lane identity inputs for one task: responsibility and config.
pub(crate) fn lane_inputs_for(task: &ProposedTask, workspace_id: &str) -> LaneInputs {
    LaneInputs {
        workspace_id: workspace_id.to_owned(),
        component_id: component_id_for_unit(&task.identity.unit_id, &task.identity.unit_path),
        task_kind: task.task_kind.clone(),
        configuration: lane_config_digest(task),
        writer_lane: writer_lane_for(&task.task_id),
    }
}

/// Lane identity digest, total over any task.
///
/// The centralized [`lane_id`] validates first; hostile inputs that fail
/// validation fall back to a plain digest over the same fields so two
/// distinct responsibilities still never collide.
pub(crate) fn lane_id_for(task: &ProposedTask, workspace_id: &str) -> String {
    let inputs = lane_inputs_for(task, workspace_id);
    if let Ok(id) = lane_id(&inputs) {
        return id;
    }
    canonical_digest(&inputs).unwrap_or_else(|_| digest_b3(b"lane_error"))
}

/// Toolchain inputs with exact component evidence, never `unreported`.
///
/// Rust hardcodes the Rust/MBX/Nextest pinned tools; tofu pins
/// `opentofu` plus the per-root provider-surface declaration (the
/// provider inputs resolve per root through the tofu adapter).
/// Neither stack may reuse the other's path or [`cache_format_id_for`].
///
/// # Errors
///
/// Returns [`ContractError`] for proposals outside a registered stack
/// or tofu kinds outside the known tokens.
pub(crate) fn toolchain_inputs_for(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<ToolchainInputs, ContractError> {
    match Stack::require_known(&task.stack_id)? {
        Stack::Mise => return super::named_checks::toolchain_inputs(task, catalog),
        Stack::Rust => {}
        Stack::Tofu => {
            let specs = catalog.tool_specs(&[PinnedTool::Opentofu]);
            return velnor_actions_tofu_core::toolchain_inputs_for_task(task, specs);
        }
    }
    let needs = tool_needs(&task.identity.compile_driver, &task.identity.test_runner);
    let mut tools = vec![PinnedTool::Rust];
    if needs.mbx {
        tools.push(PinnedTool::MrBoxington);
    }
    if needs.nextest {
        tools.push(PinnedTool::Nextest);
    }
    let mut specs = catalog.tool_specs(&tools);
    specs.sort();
    Ok(ToolchainInputs {
        tools: specs,
        components: velnor_actions_mise::PrepareRustComponents::components(),
        compile_driver: task.identity.compile_driver.clone(),
        test_runner: task.identity.test_runner.clone(),
    })
}

/// Toolchain identity digest over the centralized toolchain inputs.
///
/// # Errors
///
/// Returns [`ContractError`] for invalid toolchain inputs.
pub(crate) fn toolchain_digest_for(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    toolchain_id(&toolchain_inputs_for(task, catalog)?)
}

/// Cache-format identity for one compile driver.
///
/// The single cache format is versioned here; the typed driver admits
/// no unknown spelling, so no fallback digest can ever trigger.
pub(crate) fn cache_format_id_for(driver: CompileDriver) -> String {
    let generation = match driver {
        CompileDriver::Cargo => "1".to_owned(),
        CompileDriver::Mbx => {
            mbx_cache_generation(velnor_actions_mise::catalog::MR_BOXINGTON_VERSION)
        }
    };
    if let Ok(id) = cache_format_id(&FormatInputs {
        adapter: driver.as_str().to_owned(),
        format: CACHE_FORMAT_LABEL.to_owned(),
        generation,
    }) {
        return id;
    }
    canonical_digest(&serde_json::json!({
        "schema": "velnor-cache-format-fallback-v1",
        "adapter": driver.as_str(),
    }))
    .unwrap_or_else(|_| digest_b3(b"cache_format_error"))
}

/// Cache-format identity for tofu tasks (the tofu adapter reports it).
///
/// Same payload version as rust; the adapter slot distinguishes the
/// family. Total: the contract admits the `tofu` adapter, so the
/// fallback below never triggers.
pub(crate) fn cache_format_id_for_tofu() -> String {
    if let Ok(id) = cache_format_id(&FormatInputs {
        adapter: velnor_actions_tofu_core::STACK_ID.to_owned(),
        format: CACHE_FORMAT_LABEL.to_owned(),
        generation: "1".to_owned(),
    }) {
        return id;
    }
    canonical_digest(&serde_json::json!({
        "schema": "velnor-cache-format-fallback-v1",
        "adapter": velnor_actions_tofu_core::STACK_ID,
    }))
    .unwrap_or_else(|_| digest_b3(b"cache_format_error"))
}
