//! Plan-shape helpers for `plan-v1`: packages, metadata, and identity.

// Wired here so the FLOW-half module compiles without touching `lib.rs`.
#[path = "wire_w2.rs"]
pub(crate) mod wire_w2;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ContractError, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef, PlanGenerator, PlanPackage,
    StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, VcsInputs,
    canonical_json_bytes, digest_b3, input_digest,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::{
    Evidence, GroupExtensionInputs, STACK_ID, TaskGroup, WorkspaceRecord, adapter_entry_metadata,
};

use crate::discover::Discovery;

/// Manifest path for a manifest key.
pub(crate) fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

/// Opaque adapter metadata for one matrix entry, forwarded uninterpreted.
///
/// The Rust adapter constructs the value (legacy fields plus detected
/// driver/runner/evidence); the orchestrator only carries the bytes.
pub(crate) fn adapter_metadata(group: &TaskGroup, evidence: &[Evidence]) -> serde_json::Value {
    adapter_entry_metadata(group, evidence)
}

/// Profile evidence backing one group: its workspace sightings, if any.
pub(crate) fn evidence_for_group<'a>(
    discovery: &'a Discovery,
    group: &TaskGroup,
) -> &'a [Evidence] {
    for workspace in &discovery.workspaces {
        let owns = workspace
            .record
            .packages
            .iter()
            .any(|package| package.id == group.package_id);
        if owns {
            return &workspace.profile.evidence;
        }
    }
    &[]
}

/// Owned identity-extension inputs for one group (PAR-3.4, PAR-4.8).
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
}

impl ExtensionBundle {
    /// Borrow the bundle as adapter inputs; rerun inputs stay unknown at
    /// plan time, which is conservative for build-script packages.
    pub(crate) fn inputs(&self) -> GroupExtensionInputs<'_> {
        GroupExtensionInputs {
            package_id: &self.package_id,
            workspace_id: &self.workspace_id,
            profile: &self.profile,
            manifest: &self.manifest,
            graph_digest: &self.graph_digest,
            targets: &self.targets,
            config_digest: &self.config_digest,
            lock_digest: None,
            nextest_digest: None,
            archive_source: None,
            rerun_inputs: None,
            has_build_script: self.has_build_script,
        }
    }

    /// Tool-input config digest backing the bundle.
    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }
}

/// Orchestrator-recorded cache identities for one entry (cache §2).
pub(crate) fn cache_ids_for(
    group: &TaskGroup,
    label: &str,
    lane: u32,
    toolchain: &str,
) -> EntryCacheIds {
    EntryCacheIds {
        workspace_id: digest_b3(manifest_for_key(&group.manifest_key).as_bytes()),
        lane_id: digest_b3(lane.to_string().as_bytes()),
        platform_id: digest_b3(label.as_bytes()),
        toolchain_id: toolchain.to_owned(),
        cache_format_id: digest_b3(b"velnor-cache-format-v1"),
    }
}

/// Toolchain identity digest for one group.
pub(crate) fn toolchain_id(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    let mut tools = vec![PinnedTool::Rust];
    if group.compile_driver == "mbx" {
        tools.push(PinnedTool::MrBoxington);
    }
    let specs = catalog.tool_specs(&tools);
    Ok(digest_b3(&canonical_json_bytes(&specs)?))
}

/// Record task-cache reuse outputs on entry metadata (REUSE-3, CACHE-2.8).
pub(crate) fn record_task_cache(
    metadata: &mut serde_json::Value,
    enabled: bool,
    key: Option<&str>,
) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object.insert(
        "task_cache_enabled".to_owned(),
        serde_json::Value::Bool(enabled),
    );
    if let Some(key) = key {
        object.insert(
            "task_cache_key".to_owned(),
            serde_json::Value::String(key.to_owned()),
        );
    }
}

/// Extension bundle for one group from discovery facts.
pub(crate) fn extension_bundle(discovery: &Discovery, group: &TaskGroup) -> ExtensionBundle {
    let manifest = manifest_for_key(&group.manifest_key);
    let mut bundle = ExtensionBundle {
        package_id: group.package_id.clone(),
        workspace_id: digest_b3(b"no-workspace"),
        profile: group.configuration.clone(),
        manifest,
        graph_digest: digest_b3(b"no-workspace"),
        targets: Vec::new(),
        config_digest: tool_config_digest(discovery),
        has_build_script: false,
    };
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            let owned = package.id == group.package_id
                || (group.package_id.is_empty() && package.manifest == bundle.manifest);
            if !owned {
                continue;
            }
            bundle.workspace_id = digest_b3(workspace.record.workspace_root.as_bytes());
            bundle.graph_digest = graph_digest_for(&workspace.record);
            bundle.targets = package
                .targets
                .iter()
                .map(|target| format!("{}:{}", target.kind, target.name))
                .collect();
            bundle.has_build_script = package.has_build_script;
        }
    }
    bundle
}

/// Canonical digest over workspace members plus local-path edges.
fn graph_digest_for(record: &WorkspaceRecord) -> String {
    let mut text = record.workspace_root.clone();
    for member in &record.members {
        text.push('|');
        text.push_str(member);
    }
    for edge in &record.edges {
        let kind = match edge.kind {
            velnor_actions_rust::DepKind::Normal => "normal",
            velnor_actions_rust::DepKind::Build => "build",
            velnor_actions_rust::DepKind::Dev => "dev",
        };
        text.push('|');
        text.push_str(&edge.from);
        text.push('>');
        text.push_str(&edge.to);
        text.push(':');
        text.push_str(kind);
        text.push(':');
        text.push_str(if edge.optional { "true" } else { "false" });
        text.push(':');
        text.push_str(edge.target.as_deref().unwrap_or("-"));
    }
    digest_b3(text.as_bytes())
}

/// Config digest over observed tool-input content digests.
fn tool_config_digest(discovery: &Discovery) -> String {
    let mut joined = String::new();
    for check in &discovery.tool_checks {
        if let Some(digest) = &check.digest {
            joined.push_str(digest);
        }
    }
    digest_b3(joined.as_bytes())
}

/// Contract identity preimage for one group (CACHE-1.17, TASK-4.2).
pub(crate) struct IdentityInputs<'a> {
    /// Derived task group.
    pub(crate) group: &'a TaskGroup,
    /// Fixed argument vector.
    pub(crate) argv: &'a [String],
    /// Toolchain identity digest.
    pub(crate) toolchain_id: &'a str,
    /// Platform identity digest.
    pub(crate) platform_id: &'a str,
    /// Normalized manifest path.
    pub(crate) manifest: &'a str,
    /// Generator identity.
    pub(crate) generator: &'a PlanGenerator,
    /// Typed adapter extension.
    pub(crate) extension: StackExtension,
}

/// Input digest over the contract identity envelope.
pub(crate) fn task_identity_digest(inputs: &IdentityInputs<'_>) -> Result<String, ContractError> {
    let group = inputs.group;
    let mut dependencies = group.depends_on.clone();
    dependencies.sort();
    let mut features = group.features.clone();
    features.sort();
    let mut flags = group.target_flags.clone();
    flags.sort();
    let root = project_root_of(inputs.manifest);
    let identity = TaskIdentity {
        schema_version: 1,
        stack_id: STACK_ID.to_owned(),
        project_root: root.to_owned(),
        component_id: component_id_of(&group.package_id),
        task_kind: group.kind.as_str().to_owned(),
        task_id: group.task_id.clone(),
        argv: inputs.argv.to_vec(),
        working_dir: root.to_owned(),
        configuration: TaskConfiguration {
            target: group.target.clone(),
            profile: group.configuration.clone(),
            features,
            flags,
            task_contract: "task-execution-v1".to_owned(),
            compile_driver: group.compile_driver.clone(),
            test_runner: group.test_runner.clone(),
        },
        inputs: Vec::new(),
        dependencies,
        vcs: VcsInputs {
            commit: None,
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: inputs.toolchain_id.to_owned(),
        platform_id: inputs.platform_id.to_owned(),
        environment: BTreeMap::new(),
        output_contract: "task-report-v1".to_owned(),
        generator: TaskGenerator {
            version: inputs.generator.version.clone(),
            target: inputs.generator.target.clone(),
        },
        stack_extension: inputs.extension.clone(),
    };
    input_digest(&identity)
}

/// Project root for a manifest path; `.` for the repository root.
fn project_root_of(manifest: &str) -> &str {
    manifest
        .rsplit_once('/')
        .map_or(".", |(dir, _)| if dir.is_empty() { "." } else { dir })
}

/// Component identity: the package, or `workspace` for workspace-level groups.
pub(crate) fn component_id_of(package_id: &str) -> String {
    if package_id.is_empty() {
        "workspace".to_owned()
    } else {
        package_id.to_owned()
    }
}

/// Single executable obligation named by kind.
pub(crate) fn execute_ids(group: &TaskGroup) -> ExecuteTaskIds {
    ExecuteTaskIds {
        tasks: BTreeMap::from([(
            group.kind.as_str().to_owned(),
            ExecuteTaskRef::Single(group.task_id.clone()),
        )]),
    }
}

/// Default generator identity when the request omits it.
pub(crate) fn default_generator() -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        sha256: "0".repeat(64),
    }
}

/// Complete package inventory with selection flags.
pub(crate) fn plan_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut packages = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            let is_selected = selected.contains(package.id.as_str());
            let mut tasks: Vec<String> = discovery
                .task_groups
                .iter()
                .filter(|group| group.package_id == package.id)
                .map(|group| group.task_id.clone())
                .collect();
            tasks.sort();
            packages.push(PlanPackage {
                package_id: package.id.clone(),
                name: package.name.clone(),
                manifest: package.manifest.clone(),
                selected: is_selected,
                reasons: vec![if is_selected {
                    "selected".to_owned()
                } else {
                    "not_affected".to_owned()
                }],
                tasks,
            });
        }
    }
    packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    packages
}
