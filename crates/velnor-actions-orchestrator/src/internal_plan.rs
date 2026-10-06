//! Plan-shape helpers for `plan-v1`: packages, metadata, and identity.

// Wired here so the FLOW-half module compiles without touching `lib.rs`.
#[path = "wire_w2.rs"]
pub(crate) mod wire_w2;
// P03 identity modules live here so no `lib.rs` edit can collide.
#[path = "closure.rs"]
pub(crate) mod closure;
#[path = "closure_slots.rs"]
pub(crate) mod closure_slots;
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

#[path = "named_checks.rs"]
pub(crate) mod named_checks;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ContractError, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef, PlanGenerator, PlanPackage,
    ProposedTask, Stack, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput,
    VcsInputs, component_id_for_unit, digest_b3, input_digest,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{CompileDriver, Evidence, entry_metadata_for_task};

use crate::discover::Discovery;
use crate::internal_plan::identities::ExtensionBundle;

/// # Errors
pub(crate) fn tofu_extension_for(
    task: &ProposedTask,
    root: &std::path::Path,
    bundle: &ExtensionBundle,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<velnor_actions_tofu::TofuTaskIdentityExtension, ContractError> {
    let normalized = velnor_actions_tofu::normalized_root_for_proposal(task)?;
    let inputs = velnor_actions_tofu::TofuGroupExtensionInputs {
        unit_id: &task.identity.unit_id,
        workspace_id: bundle.workspace_id(),
        profile: &task.configuration,
        manifest: &task.identity.unit_path,
        graph_digest: bundle.graph_digest(),
        root: normalized,
        config_digest: bundle.config_digest(),
        lock_digest: velnor_actions_tofu::lock_slot_at_root(root, &task.identity.unit_path, reads),
    };
    velnor_actions_tofu::extension_for_proposal(task, &inputs)
}

pub(crate) const CLOSURE_INPUT_PATH: &str = "velnor/input-closure";

/// # Errors
pub(crate) fn adapter_metadata(
    task: &ProposedTask,
    evidence: &[Evidence],
) -> Result<serde_json::Value, ContractError> {
    match Stack::require_known(&task.stack_id)? {
        Stack::Tofu => {
            let ids: Vec<String> = evidence
                .iter()
                .map(velnor_actions_rust::tasks::evidence_id)
                .collect();
            velnor_actions_tofu::entry_metadata_for_task(task, &ids)
        }
        Stack::Rust => entry_metadata_for_task(task, evidence),
        Stack::Mise => Err(ContractError::identity(
            "check",
            "discovered_metadata_required",
        )),
    }
}

pub(crate) fn evidence_for_group<'a>(
    discovery: &'a Discovery,
    task: &ProposedTask,
) -> &'a [Evidence] {
    for workspace in &discovery.workspaces {
        let owns = workspace
            .record
            .packages
            .iter()
            .any(|package| package.id == task.identity.unit_id);
        if owns {
            return &workspace.profile.evidence;
        }
    }
    &[]
}

pub(crate) fn nextest_config_for(discovery: &Discovery, task: &ProposedTask) -> Option<String> {
    for workspace in &discovery.workspaces {
        let owns = workspace
            .record
            .packages
            .iter()
            .any(|package| package.id == task.identity.unit_id);
        if owns {
            return workspace.profile.nextest_config.clone();
        }
    }
    None
}

/// schedule ordinal (ordinals are removed: no lane parameter exists).
/// Platform identity binds runner image evidence; every digest is
/// validated at construction, never stored raw.
///
/// # Errors
///
/// Returns [`ContractError`] when any digest fails validation.
pub(crate) fn cache_ids_for(
    task: &ProposedTask,
    label: &str,
    toolchain: &str,
) -> Result<EntryCacheIds, ContractError> {
    if Stack::require_known(&task.stack_id)? == Stack::Mise {
        return Err(ContractError::identity(
            "cache",
            "opaque_check_cache_forbidden",
        ));
    }
    let workspace_id = digest_b3(task.identity.unit_path.as_bytes());
    let format_id = if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        identities::cache_format_id_for_tofu()
    } else {
        identities::cache_format_id_for(CompileDriver::parse(&task.identity.compile_driver)?)
    };
    EntryCacheIds::new(
        &workspace_id,
        &identities::lane_id_for(task, &workspace_id),
        &identities::platform_id_for_group(label, task)?,
        toolchain,
        &format_id,
    )
}

/// Isolated `CARGO_TARGET_DIR` for one lane identity (P03-7).
///
/// Production wiring helper: generated legs set `CARGO_TARGET_DIR` to
/// this path so concurrent writers never share a target dir. The path
/// derives from the responsibility-based lane digest, never an ordinal.
pub(crate) fn target_dir_for_lane_id(lane_id: &str) -> String {
    velnor_actions_workflow_renderer::lane_target::target_dir_for_lane(lane_id)
}

/// Toolchain identity digest for one task.
pub(crate) fn toolchain_id(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    identities::toolchain_digest_for(task, catalog)
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

/// Contract identity preimage for one task (CACHE-1.17, TASK-4.2).
pub(crate) struct IdentityInputs<'a> {
    /// Adapter task proposal.
    pub(crate) task: &'a ProposedTask,
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
    let task = inputs.task;
    let mut dependencies = task.depends_on.clone();
    dependencies.sort();
    let mut features = task.identity.features.clone();
    features.sort();
    let mut flags = task.identity.flags.clone();
    flags.sort();
    let root = if task.stack_id == velnor_actions_tofu::STACK_ID {
        velnor_actions_tofu::normalized_root_for_proposal(task)?;
        task.identity.project_root.as_str()
    } else if Stack::from_id(&task.stack_id) == Some(Stack::Mise) {
        task.identity.project_root.as_str()
    } else {
        project_root_of(inputs.manifest)
    };
    let identity = TaskIdentity {
        schema_version: 1,
        stack_id: task.stack_id.clone(),
        project_root: root.to_owned(),
        component_id: component_id_for_unit(&task.identity.unit_id, inputs.manifest),
        task_kind: task.task_kind.clone(),
        task_id: task.task_id.clone(),
        argv: inputs.argv.to_vec(),
        working_dir: root.to_owned(),
        configuration: TaskConfiguration {
            target: task.identity.target.clone(),
            profile: task.configuration.clone(),
            features,
            flags,
            task_contract: "task-execution-v1".to_owned(),
            compile_driver: task.identity.compile_driver.clone(),
            test_runner: task.identity.test_runner.clone(),
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
        environment: task.identity.environment.clone(),
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

/// Single executable obligation named by kind.
pub(crate) fn execute_ids(task: &ProposedTask) -> ExecuteTaskIds {
    ExecuteTaskIds {
        tasks: BTreeMap::from([(
            task.task_kind.clone(),
            ExecuteTaskRef::Single(task.task_id.clone()),
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
#[cfg(test)]
pub(crate) fn default_generator() -> PlanGenerator {
    default_generator_with_phase_timings(None)
}

pub(crate) fn default_generator_with_phase_timings(
    phases: Option<&mut crate::internal::phase_timing::PlanPhaseTimings>,
) -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: snapshot::map_release_triple(std::env::consts::ARCH, std::env::consts::OS),
        sha256: crate::cover_identity::generator::current_exe_sha256_with_phase_timings(phases)
            .unwrap_or_else(|| snapshot::UNRESOLVED_GENERATOR_SHA.to_owned()),
    }
}

/// Complete package inventory with selection flags.
///
/// Rust rows enumerate workspace packages; tofu rows enumerate
/// selected tofu projects by root key, so an untouched root stays
/// distinguishable (`not_affected`) instead of vanishing.
pub(crate) fn plan_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut packages = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            packages.push(package_row(
                &package.id,
                &package.name,
                &package.manifest,
                discovery,
                selected,
            ));
        }
    }
    packages.extend(tofu_packages(discovery, selected));
    packages.extend(discovery.mise_checks.iter().map(|item| {
        package_row(
            &item.proposal.identity.unit_id,
            &item.check.id,
            &item.proposal.identity.unit_path,
            discovery,
            selected,
        )
    }));
    packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    packages
}

/// One inventory row: selection flag plus sorted task IDs.
fn package_row(
    package_id: &str,
    name: &str,
    manifest: &str,
    discovery: &Discovery,
    selected: &BTreeSet<&str>,
) -> PlanPackage {
    let is_selected = selected.contains(package_id);
    let mut tasks: Vec<String> = discovery
        .proposals
        .iter()
        .filter(|task| task.identity.unit_id == package_id)
        .map(|task| task.task_id.clone())
        .collect();
    tasks.sort();
    PlanPackage {
        package_id: package_id.to_owned(),
        name: name.to_owned(),
        manifest: manifest.to_owned(),
        selected: is_selected,
        reasons: vec![if is_selected {
            "selected".to_owned()
        } else {
            "not_affected".to_owned()
        }],
        tasks,
    }
}

/// Tofu inventory rows: one per selected project, keyed by root key.
fn tofu_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut roots = crate::select_tofu::tofu_selected_roots(&discovery.statuses);
    roots.dedup();
    roots
        .iter()
        .map(|root| {
            let key = velnor_actions_tofu::key_for_root(root);
            let display = velnor_actions_tofu::display_for_root(root);
            package_row(&key, &display, &display, discovery, selected)
        })
        .collect()
}
