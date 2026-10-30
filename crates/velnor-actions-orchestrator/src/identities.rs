// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use velnor_actions_contract::cachekey::{
    FormatInputs, LaneInputs, ToolchainInputs, cache_format_id, lane_id, mbx_cache_generation,
    toolchain_id,
};
use velnor_actions_contract::{
    ContractError, ProposedTask, Stack, component_id_for_unit, digest_b3,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::{CompileDriver, DepKind, WorkspaceRecord, tool_needs};

use super::snapshot::{ExecutionSnapshot, canonical_digest, platform_id_for};
use crate::discover::Discovery;
use crate::toolcheck::ToolInputCheck;

const CACHE_FORMAT_LABEL: &str = "velnor-cache-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotGraph {
    pub(crate) members: Vec<String>,
    pub(crate) edges: Vec<SnapshotEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotEdge {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: String,
    pub(crate) optional: bool,
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
    package_id: String,
    workspace_id: String,
    profile: String,
    manifest: String,
    graph_digest: String,
    targets: Vec<String>,
    config_digest: String,
    has_build_script: bool,
    lock_digest: velnor_actions_rust::tasks::DigestSlot,
    nextest_digest: velnor_actions_rust::tasks::DigestSlot,
    rerun_inputs: Option<Vec<String>>,
}

impl ExtensionBundle {
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

    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }

    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(crate) fn graph_digest(&self) -> &str {
        &self.graph_digest
    }
}

pub(crate) fn extension_bundle_with_snapshot(
    snapshot: &ExecutionSnapshot,
    _discovery: &Discovery,
    task: &ProposedTask,
    root: Option<&std::path::Path>,
    nextest_config: Option<&str>,
) -> ExtensionBundle {
    let manifest = task.identity.unit_path.clone();
    let workspace_id = snapshot.workspace_id_for(&task.identity.unit_id, &manifest);
    let graph_digest = snapshot.graph_digest_for(&task.identity.unit_id, &manifest);
    let (targets, has_build_script) = snapshot
        .package_meta_for(&task.identity.unit_id, &manifest)
        .cloned()
        .unwrap_or_default();
    let mut bundle = ExtensionBundle {
        package_id: task.identity.unit_id.clone(),
        workspace_id,
        profile: task.configuration.clone(),
        manifest,
        graph_digest,
        targets,
        config_digest: snapshot.config_digest().to_owned(),
        has_build_script,
        lock_digest: velnor_actions_rust::tasks::DigestSlot::Unknown("no_checkout".to_owned()),
        nextest_digest: velnor_actions_rust::tasks::DigestSlot::Unknown("no_checkout".to_owned()),
        rerun_inputs: None,
    };
    if !bundle.has_build_script {
        bundle.rerun_inputs = Some(Vec::new());
    }
    if Stack::from_id(&task.stack_id) != Some(Stack::Rust) {
        bundle.lock_digest =
            velnor_actions_rust::tasks::DigestSlot::Unknown("tofu_adapter_owned".to_owned());
        bundle.nextest_digest =
            velnor_actions_rust::tasks::DigestSlot::Unknown("tofu_adapter_owned".to_owned());
    } else if let Some(root) = root {
        bundle.lock_digest = snapshot.rust_lock_digest(root, &bundle.manifest);
        bundle.nextest_digest = snapshot.rust_nextest_digest(root, nextest_config);
    }
    bundle
}

pub(crate) fn platform_id_for_group(
    label: &str,
    task: &ProposedTask,
) -> Result<String, ContractError> {
    let stack = Stack::require_known(&task.stack_id)?;
    if stack == Stack::Mise {
        let label = task.runner_profile.as_str();
        if let Some(host) = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
            .map(velnor_actions_contract::ReleaseTarget::triple)
            && host != task.identity.target
        {
            return Err(ContractError::identity(
                "runner_label",
                "runner_target_mismatch",
            ));
        }
        return platform_id_for(label, &task.identity.target);
    }
    let host = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
        .filter(|target| *target == velnor_actions_contract::ReleaseTarget::LinuxX86_64)
        .map(velnor_actions_contract::ReleaseTarget::triple)
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

pub(crate) fn lane_inputs_for(task: &ProposedTask, workspace_id: &str) -> LaneInputs {
    LaneInputs {
        workspace_id: workspace_id.to_owned(),
        component_id: component_id_for_unit(&task.identity.unit_id, &task.identity.unit_path),
        task_kind: task.task_kind.clone(),
        configuration: lane_config_digest(task),
        writer_lane: writer_lane_for(&task.task_id),
    }
}

pub(crate) fn lane_id_for(task: &ProposedTask, workspace_id: &str) -> String {
    let inputs = lane_inputs_for(task, workspace_id);
    if let Ok(id) = lane_id(&inputs) {
        return id;
    }
    canonical_digest(&inputs).unwrap_or_else(|_| digest_b3(b"lane_error"))
}

pub(crate) fn toolchain_inputs_for(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<ToolchainInputs, ContractError> {
    match Stack::require_known(&task.stack_id)? {
        Stack::Mise => return super::named_checks::toolchain_inputs(task, catalog),
        Stack::Native => {
            return Err(ContractError::identity(
                "native_check",
                "native_toolchain_requires_plan_derivation",
            ));
        }
        Stack::Rust => {}
        Stack::Tofu => {
            let specs = catalog.tool_specs(&[PinnedTool::Opentofu]);
            return velnor_actions_tofu::toolchain_inputs_for_task(task, specs);
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
        selected_test_tools: Vec::new(),
    })
}

pub(crate) fn toolchain_digest_for(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    toolchain_id(&toolchain_inputs_for(task, catalog)?)
}

pub(crate) fn cache_format_id_for(driver: CompileDriver, mbx_version: &str) -> String {
    let generation = match driver {
        CompileDriver::Cargo => "1".to_owned(),
        CompileDriver::Mbx => mbx_cache_generation(mbx_version),
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

pub(crate) fn cache_format_id_for_tofu() -> String {
    if let Ok(id) = cache_format_id(&FormatInputs {
        adapter: velnor_actions_tofu::STACK_ID.to_owned(),
        format: CACHE_FORMAT_LABEL.to_owned(),
        generation: "1".to_owned(),
    }) {
        return id;
    }
    canonical_digest(&serde_json::json!({
        "schema": "velnor-cache-format-fallback-v1",
        "adapter": velnor_actions_tofu::STACK_ID,
    }))
    .unwrap_or_else(|_| digest_b3(b"cache_format_error"))
}
