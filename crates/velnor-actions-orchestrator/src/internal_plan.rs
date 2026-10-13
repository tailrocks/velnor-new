//! Plan-shape helpers for `plan-v1`: packages, metadata, and identity.

// Wired here so the FLOW-half module compiles without touching `lib.rs`.
#[path = "wire_w2.rs"]
pub(crate) mod wire_w2;
// P03 identity modules live here so no `lib.rs` edit can collide.
#[path = "closure.rs"]
pub(crate) mod closure;
#[cfg(test)]
#[path = "closure_tests.rs"]
mod closure_tests;
#[path = "identities.rs"]
pub(crate) mod identities;
#[cfg(test)]
#[path = "identities_tests.rs"]
mod identities_tests;
#[cfg(test)]
#[path = "internal_plan_tests.rs"]
mod internal_plan_tests;
#[path = "snapshot.rs"]
pub(crate) mod snapshot;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ContractError, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef, PlanGenerator, PlanPackage,
    StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput, VcsInputs,
    digest_b3, input_digest,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{
    Evidence, STACK_ID, TaskGroup, adapter_entry_metadata, cargo_payload_env,
};

use crate::discover::Discovery;

/// Synthetic identity-input path carrying the input-closure digest.
///
/// The closure digest is content, not a file: this path never resolves
/// against the checkout. It only names the [`TaskInput`] slot so the
/// closure binds into `input_digest` through the standard envelope.
pub(crate) const CLOSURE_INPUT_PATH: &str = "velnor/input-closure";

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

/// Nextest config path backing one group, if the profile names one.
pub(crate) fn nextest_config_for(discovery: &Discovery, group: &TaskGroup) -> Option<String> {
    for workspace in &discovery.workspaces {
        let owns = workspace
            .record
            .packages
            .iter()
            .any(|package| package.id == group.package_id);
        if owns {
            return workspace.profile.nextest_config.clone();
        }
    }
    None
}

/// Orchestrator-recorded cache identities for one entry (cache §2).
///
/// Lane identity derives from responsibility and config, never from a
/// schedule ordinal (ordinals are removed: no lane parameter exists).
/// Platform identity binds runner image evidence; every digest is
/// validated at construction, never stored raw.
///
/// # Errors
///
/// Returns [`ContractError`] when any digest fails validation.
pub(crate) fn cache_ids_for(
    group: &TaskGroup,
    label: &str,
    toolchain: &str,
) -> Result<EntryCacheIds, ContractError> {
    let manifest = manifest_for_key(&group.manifest_key);
    let workspace_id = digest_b3(manifest.as_bytes());
    EntryCacheIds::new(
        &workspace_id,
        &identities::lane_id_for(group, &workspace_id),
        &identities::platform_id_for_group(label, group),
        toolchain,
        &identities::cache_format_id_for(&group.compile_driver),
    )
}

/// Isolated `CARGO_TARGET_DIR` for one lane identity (P03-7).
///
/// Production wiring helper: generated legs set `CARGO_TARGET_DIR` to
/// this path so concurrent writers never share a target dir. The path
/// derives from the responsibility-based lane digest, never an ordinal.
pub(crate) fn target_dir_for_lane_id(lane_id: &str) -> String {
    velnor_actions_workflow_renderer::steps::target_dir_for_lane(lane_id)
}

/// Toolchain identity digest for one group.
pub(crate) fn toolchain_id(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    identities::toolchain_digest_for(group, catalog)
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
    /// Canonical digest over the task's complete input closure.
    pub(crate) closure_digest: &'a str,
}

/// Input digest over the contract identity envelope.
///
/// The envelope binds argv, configuration, toolchain, platform, the
/// behavior-affecting environment contract, the full adapter extension,
/// and the canonical input-closure digest resolved against the checkout:
/// a source edit flips `input_digest` even when the changed-work hint
/// misses it.
pub(crate) fn task_identity_digest(inputs: &IdentityInputs<'_>) -> Result<String, ContractError> {
    let group = inputs.group;
    let mut dependencies = group.depends_on.clone();
    dependencies.sort();
    let mut features = group.features.clone();
    features.sort();
    let mut flags = group.target_flags.clone();
    flags.sort();
    let root = project_root_of(inputs.manifest);
    let environment: BTreeMap<String, String> = cargo_payload_env(group.kind)
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let identity = TaskIdentity {
        schema_version: 1,
        stack_id: STACK_ID.to_owned(),
        project_root: root.to_owned(),
        component_id: snapshot::normalized_component_id(&group.package_id, inputs.manifest),
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
        inputs: vec![TaskInput {
            path: CLOSURE_INPUT_PATH.to_owned(),
            digest: inputs.closure_digest.to_owned(),
        }],
        dependencies,
        vcs: VcsInputs {
            commit: None,
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: inputs.toolchain_id.to_owned(),
        platform_id: inputs.platform_id.to_owned(),
        environment,
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
///
/// The target is a release triple when the host maps to one; the SHA is
/// the running executable's real SHA-256 (release-comparable) when its
/// bytes are readable, else the explicit unresolved marker (never
/// all-zero, never a `b3-` hash: native hashes are incomparable with
/// release pins and fail closed).
pub(crate) fn default_generator() -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: snapshot::map_release_triple(std::env::consts::ARCH, std::env::consts::OS),
        sha256: crate::cover_identity::generator::current_exe_sha256()
            .unwrap_or_else(|| snapshot::UNRESOLVED_GENERATOR_SHA.to_owned()),
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
