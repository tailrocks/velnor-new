// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "closure.rs"]
pub(crate) mod closure;
#[path = "wire_w2.rs"]
pub(crate) mod wire_w2;

#[path = "identities.rs"]
pub(crate) mod identities;

#[path = "snapshot.rs"]
pub(crate) mod snapshot;

#[path = "named_checks.rs"]
pub(crate) mod named_checks;
#[path = "native_homebrew_checks.rs"]
pub(crate) mod native_homebrew_checks;
#[path = "internal_plan_packages.rs"]
mod packages;
pub(crate) use packages::plan_packages;

use std::collections::BTreeMap;

use velnor_actions_contract::{
    ContractError, EntryCacheIds, ExecuteTaskIds, ExecuteTaskRef, PlanGenerator, ProposedTask,
    Stack, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput, VcsInputs,
    component_id_for_unit, digest_b3, input_digest,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{CompileDriver, Evidence, entry_metadata_for_task};

use crate::discover::Discovery;
use crate::internal_plan::identities::ExtensionBundle;

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
        Stack::Native => Err(ContractError::identity(
            "native_check",
            "native_metadata_requires_plan_derivation",
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

pub(crate) fn cache_ids_for(
    task: &ProposedTask,
    label: &str,
    toolchain: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<EntryCacheIds, ContractError> {
    if matches!(
        Stack::require_known(&task.stack_id)?,
        Stack::Mise | Stack::Native
    ) {
        return Err(ContractError::identity(
            "cache",
            "opaque_check_cache_forbidden",
        ));
    }
    let workspace_id = digest_b3(task.identity.unit_path.as_bytes());
    let format_id = if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        identities::cache_format_id_for_tofu()
    } else {
        identities::cache_format_id_for(
            CompileDriver::parse(&task.identity.compile_driver)?,
            catalog.version(velnor_actions_mise::PinnedTool::MrBoxington),
        )
    };
    EntryCacheIds::new(
        &workspace_id,
        &identities::lane_id_for(task, &workspace_id),
        &identities::platform_id_for_group(label, task)?,
        toolchain,
        &format_id,
    )
}

pub(crate) fn target_dir_for_lane_id(lane_id: &str) -> String {
    velnor_actions_workflow_renderer::lane_target::target_dir_for_lane(lane_id)
}

pub(crate) fn toolchain_id(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    identities::toolchain_digest_for(task, catalog)
}

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

pub(crate) struct IdentityInputs<'a> {
    pub(crate) task: &'a ProposedTask,
    pub(crate) argv: &'a [String],
    pub(crate) toolchain_id: &'a str,
    pub(crate) platform_id: &'a str,
    pub(crate) manifest: &'a str,
    pub(crate) generator: &'a PlanGenerator,
    pub(crate) extension: StackExtension,
    pub(crate) closure_digest: &'a str,
}

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
    } else if Stack::from_id(&task.stack_id) == Some(Stack::Native) {
        "."
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

fn project_root_of(manifest: &str) -> &str {
    manifest
        .rsplit_once('/')
        .map_or(".", |(dir, _)| if dir.is_empty() { "." } else { dir })
}

pub(crate) fn execute_ids(task: &ProposedTask) -> ExecuteTaskIds {
    ExecuteTaskIds {
        tasks: BTreeMap::from([(
            task.task_kind.clone(),
            ExecuteTaskRef::Single(task.task_id.clone()),
        )]),
    }
}

pub(crate) fn default_generator_with_phase_timings(
    phases: Option<&mut crate::internal::phase_timing::PlanPhaseTimings>,
) -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: snapshot::map_release_triple(std::env::consts::ARCH, std::env::consts::OS),
        digest_b3: crate::generator::current_exe_sha256_with_phase_timings(phases)
            .unwrap_or_else(|| snapshot::UNRESOLVED_GENERATOR_SHA.to_owned()),
    }
}
