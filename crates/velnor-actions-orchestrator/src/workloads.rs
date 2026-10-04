//! Closed native validation obligations; repository commands never become policy.

use std::collections::BTreeMap;
use std::ffi::OsString;
use velnor_actions_contract::config::{WorkloadConfig, WorkloadKind};
use velnor_actions_contract::propose::IdentityInputs;
use velnor_actions_contract::{
    CachePolicy, ContractError, FileIndex, ProposedTask, ResourceClass, ResourceDemand, Stack,
    VelnorConfig,
};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

#[path = "workloads_cache.rs"]
pub(crate) mod cache;
#[path = "workloads_cache_eligibility.rs"]
pub(crate) mod cache_eligibility;
#[path = "workloads_cache_sources.rs"]
pub(crate) mod cache_sources;
#[path = "workloads_desktop.rs"]
mod desktop;
#[path = "workloads_evidence.rs"]
mod evidence;
#[path = "workloads_host.rs"]
mod host;
#[path = "workloads_identity_recipe.rs"]
pub(crate) mod identity_recipe;
#[path = "workloads_jobs.rs"]
mod jobs;
#[path = "workloads_metadata.rs"]
pub(crate) mod metadata;
#[path = "workloads_native_descriptor.rs"]
pub(crate) mod native_descriptor;
#[path = "workloads_env_node.rs"]
mod node_env;
#[path = "workloads_operations.rs"]
mod operations;
#[path = "required_workloads.rs"]
pub(crate) mod required;
#[path = "workloads_security.rs"]
mod security;
pub(crate) use host::{argv_for_runner, host_for_runner, toolchain_id_for_runner};
pub(crate) use jobs::{prepare_step, runner_for_model, runner_for_task};
pub(crate) use node_env::{
    env_for_argv as node_env_for_argv, isolation_prefix as node_isolation_prefix,
    materialization_step as node_materialization_step,
};
use operations::phases;
pub(crate) use operations::{kind_id, rank, step_name, tools};
/// Closed native adapters that require an isolated Rust toolchain.
pub(crate) fn requires_rust(kind: &str) -> bool {
    security::requires_rust(kind)
        || matches!(kind, "native_xcode_project_ci" | "native_swift_package_ci")
}

/// Select the closed desktop compiler role only for its native operations.
pub(crate) fn catalog_for_configuration(
    catalog: &ToolCatalog,
    configuration: &str,
) -> Result<ToolCatalog, velnor_actions_mise::MiseError> {
    if matches!(
        configuration,
        "native_xcode_project_ci" | "native_swift_package_ci"
    ) {
        catalog.for_native_kind(configuration)
    } else {
        Ok(catalog.clone())
    }
}

/// Materialize configured native obligations through a closed command catalog.
pub(crate) fn derive(
    config: &VelnorConfig,
    index: &FileIndex,
) -> Result<Vec<ProposedTask>, OrchestratorError> {
    evidence::qualify(config, index)?;
    let mut tasks = Vec::new();
    for workload in &config.stacks.workloads {
        validate_evidence(workload, index)?;
        security::validate_evidence(workload, index)?;
        let mut previous = None;
        for (phase, payload) in phases(workload)? {
            let mut task = proposal(workload, phase, payload, previous.take());
            if let Some(descriptor) =
                native_descriptor::from_workload(workload, phase, &task.payload)?
            {
                task.identity.environment.insert(
                    native_descriptor::NATIVE_VALIDATION_DESCRIPTOR_KEY.to_owned(),
                    String::from_utf8(velnor_actions_contract::canonical_json_bytes(&descriptor)?)
                        .map_err(|_| crate::internal::internal("native_descriptor_not_utf8"))?,
                );
            }
            if matches!(
                workload.kind,
                WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi
            ) {
                task.identity.project_root = ".".to_owned();
                if let Some(profile) = &workload.native_desktop {
                    task.identity.environment.insert(
                        identity_recipe::NATIVE_DESKTOP_PROFILE_KEY.to_owned(),
                        String::from_utf8(velnor_actions_contract::canonical_json_bytes(profile)?)
                            .map_err(|_| crate::internal::internal("desktop_profile_not_utf8"))?,
                    );
                }
            }
            if let Some(gradle) = &workload.gradle {
                task.identity.environment.insert(
                    metadata::POSTGRES_FIXTURE_KEY.to_owned(),
                    String::from_utf8(velnor_actions_contract::canonical_json_bytes(
                        &gradle.postgres,
                    )?)
                    .map_err(|_| crate::internal::internal("postgres_fixture_not_utf8"))?,
                );
            }
            let candidates = match task.configuration.as_str() {
                "node_ci" => {
                    cache_eligibility::source_candidates(index, &task.identity.project_root)
                        .map(|sources| ("VELNOR_NATIVE_NPM_SOURCE_CANDIDATES", sources))
                }
                "bun_ci" => {
                    cache::bun::sources::source_candidates(index, &task.identity.project_root)
                        .map(|sources| ("VELNOR_NATIVE_BUN_SOURCE_CANDIDATES", sources))
                }
                _ => None,
            };
            if let Some((key, candidates)) = candidates {
                task.identity.environment.insert(
                    key.to_owned(),
                    serde_json::to_string(&candidates).map_err(|error| {
                        crate::internal::internal(&format!("native_source_candidates:{error}"))
                    })?,
                );
            }
            task.validate()?;
            previous = Some(task.task_id.clone());
            tasks.push(task);
        }
    }
    required::validate(index.root(), config, &tasks)?;
    Ok(tasks)
}

fn validate_evidence(
    workload: &WorkloadConfig,
    index: &FileIndex,
) -> Result<(), OrchestratorError> {
    let root = workload.root.as_str();
    if matches!(
        workload.kind,
        WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi
    ) {
        desktop::validate_evidence(workload, index)?;
        return Err(crate::internal::internal(
            "desktop_source_bound_primitive_qualification_pending",
        ));
    }
    if matches!(
        workload.kind,
        WorkloadKind::GradleCheck | WorkloadKind::GradleDatabaseCheck
    ) {
        return Err(crate::internal::internal(
            "gradle_exact_wrapper_authority_pending:9.5.1",
        ));
    }
    if workload.kind == WorkloadKind::HomebrewAudit {
        return Err(crate::internal::internal(
            "homebrew_exact_tool_authority_pending",
        ));
    }
    if workload.kind == WorkloadKind::PackageUpdateFixture {
        return Err(crate::internal::internal(
            "package_update_source_bound_fixture_pending",
        ));
    }
    let file = match workload.kind {
        WorkloadKind::DockerBuild => Some("Dockerfile"),
        WorkloadKind::BunCi | WorkloadKind::NodeCi => Some("package.json"),
        WorkloadKind::SwiftTest => Some("Package.swift"),
        WorkloadKind::CargoAudit
        | WorkloadKind::CargoDeny
        | WorkloadKind::TuiNoDefaultGraph
        | WorkloadKind::TuiXtaskPolicy
        | WorkloadKind::TuiXtaskDeps
        | WorkloadKind::TuiXtaskPackage => Some("Cargo.toml"),
        _ => None,
    };
    for relative in workload.inputs.iter().chain(&workload.paths) {
        if !index.contains(relative) {
            return Err(crate::internal::internal(&format!(
                "workload_input_missing:{relative}"
            )));
        }
    }
    if let Some(file) = file {
        let path = if root == "." {
            file.to_owned()
        } else {
            format!("{root}/{file}")
        };
        if !index.contains(&path) {
            return Err(crate::internal::internal(&format!(
                "workload_evidence_missing:{path}"
            )));
        }
    }
    velnor_actions_native::node::validate_evidence(workload, index).map_err(
        |error| match error {
            ContractError::InvalidIdentity { problem, .. } => crate::internal::internal(&problem),
            other => OrchestratorError::from(other),
        },
    )?;
    Ok(())
}

fn proposal(
    workload: &WorkloadConfig,
    phase: &str,
    payload: Vec<String>,
    previous: Option<String>,
) -> ProposedTask {
    let root = workload.root.as_str().to_owned();
    let mut inputs = workload.inputs.clone();
    inputs.extend(workload.paths.iter().cloned());
    inputs.sort();
    inputs.dedup();
    ProposedTask {
        task_id: format!(
            "stack/workload/{}/{phase}/{}",
            workload.name,
            kind_id(workload.kind)
        ),
        stack_id: Stack::Workload.id().to_owned(),
        component_id: workload.name.clone(),
        task_kind: phase.to_owned(),
        configuration: kind_id(workload.kind).to_owned(),
        depends_on: previous.into_iter().collect(),
        gated_by: Vec::new(),
        reads: vec!["**".to_owned()],
        writes: vec![format!("{root}/**")],
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: true,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: format!("workload:{}", workload.name),
            unit_key: workload.name.clone(),
            unit_path: root.clone(),
            project_root: root,
            target: if workload.kind == WorkloadKind::SwiftTest {
                "aarch64-apple-darwin"
            } else {
                "host"
            }
            .to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "native".to_owned(),
            test_runner: "native".to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: inputs,
            undeclared_reads: true,
        },
        payload: payload.into_iter().map(OsString::from).collect(),
        display_name: workload.name.clone(),
        uses_clock: true,
        uses_random: true,
        no_targets: false,
        runner_profile: "native".to_owned(),
    }
}

/// Reject a native vector without the emitted runner's host.
pub(crate) fn argv(
    _task: &ProposedTask,
    _catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    Err(OrchestratorError::Contract {
        problem: "native_host_required:use_argv_for_runner".to_owned(),
    })
}

/// Reject a native toolchain identity without the emitted runner's host.
pub(crate) fn toolchain_id(
    _task: &ProposedTask,
    _catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    Err(ContractError::identity(
        "native_host",
        "native_host_required:use_toolchain_id_for_runner",
    ))
}

#[cfg(test)]
#[path = "workloads_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workloads_pending_tests.rs"]
mod pending_tests;

#[cfg(test)]
#[path = "workloads_package_tests.rs"]
mod package_tests;
