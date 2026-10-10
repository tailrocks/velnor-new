//! Resolve native build tasks against the exact checked-in Mise source.

use std::collections::BTreeMap;

use velnor_actions_contract::{BuildTask, VelnorConfig, WorkflowTask};
use velnor_actions_mise::catalog::{MR_BOXINGTON_VERSION, RUST_VERSION};
use velnor_actions_workflow_renderer::verification_jobs::{BuildTaskPolicy, BuildTaskTool};

use crate::build_task_tools::resolve_selected_tools;
use crate::native_tool_input::{NativeMiseConfig, NativeToolSource};
use crate::native_tool_lock::rust_toolchain_options;
use crate::pins::resolve_build_task_mise_setup;
use crate::toolcheck::{ToolInputCheck, ToolParse};
use crate::{OrchestratorError, discover::Discovery};
#[path = "build_task_lock_validation.rs"]
mod build_task_lock_validation;
use build_task_lock_validation::{
    source_task_tool_requests, validate_lock_shape, validate_source_lock_subset,
    validate_source_task_lock_requests,
};

/// Resolve each task against source-bound Mise files and the fixed runner pin.
pub(crate) fn policies(
    config: &VelnorConfig,
    discovery: &Discovery,
) -> Result<Vec<BuildTaskPolicy>, OrchestratorError> {
    let build_tasks = config
        .workflow
        .tasks
        .iter()
        .filter_map(|task| match task {
            WorkflowTask::Build(build) => Some(build),
            WorkflowTask::Verification(_) | WorkflowTask::NativeImage(_) => None,
        })
        .collect::<Vec<_>>();
    if build_tasks.is_empty() {
        return Ok(Vec::new());
    }
    let rust = checked_source(&discovery.tool_checks, "rust-toolchain.toml")?;
    let rust_version = rust
        .values
        .get("channel")
        .filter(|version| exact_version(version))
        .ok_or_else(|| failure("build_task_rust_toolchain"))?;
    let rust_lock_options = rust_toolchain_options(&rust.values)
        .ok_or_else(|| failure("build_task_rust_toolchain_options"))?;
    let NativeToolSource::RustToolchain = &rust
        .native
        .as_ref()
        .ok_or_else(|| failure("build_task_rust_toolchain"))?
        .source
    else {
        return Err(failure("build_task_rust_toolchain"));
    };

    let mise = checked_source(&discovery.tool_checks, "mise.toml")?;
    let NativeToolSource::MiseConfig(mise_config) = &mise
        .native
        .as_ref()
        .ok_or_else(|| failure("build_task_mise_config"))?
        .source
    else {
        return Err(failure("build_task_mise_config"));
    };
    let lock = checked_source(&discovery.tool_checks, "mise.lock")?;
    let NativeToolSource::MiseLock(mise_lock) = &lock
        .native
        .as_ref()
        .ok_or_else(|| failure("build_task_mise_lock"))?
        .source
    else {
        return Err(failure("build_task_mise_lock"));
    };

    validate_config_shape(mise_config, rust_version)?;
    validate_lock_shape(mise_lock)?;
    let root_lock_sha256 = native_sha256(lock)?;
    let root_rust_sha256 = native_sha256(rust)?;
    let mut source_digests = Vec::with_capacity(build_tasks.len());
    for task in &build_tasks {
        task.validate(".velnor/config.toml")
            .map_err(|_| failure("build_task_contract"))?;
        let task_config_input = checked_source(&discovery.tool_checks, &task.source.mise_config)?;
        let NativeToolSource::MiseConfig(task_config) = &task_config_input
            .native
            .as_ref()
            .ok_or_else(|| failure("build_task_mise_task_config"))?
            .source
        else {
            return Err(failure("build_task_mise_task_config"));
        };
        validate_task_source_shape(&task.source.mise_config, task_config)?;
        let mut merged_tasks = mise_config.clone();
        merged_tasks.tasks.extend(task_config.tasks.clone());
        let local_task_tools =
            crate::native_mise_tasks::selected_build_task_tools(&merged_tasks, &task.mise_task)
                .map_err(|problem| failure(build_task_graph_problem(problem)))?;
        let selected_tools = resolve_selected_tools(
            task,
            mise_config,
            mise_lock,
            rust_version,
            &rust_lock_options,
        )?;
        validate_task_tool_selection(task, &local_task_tools, &selected_tools)?;
        let source_task_tools = source_task_tool_requests(task_config)?;
        for (key, version) in &source_task_tools {
            validate_task_tool_selection(
                task,
                &BTreeMap::from([(key.clone(), version.clone())]),
                &selected_tools,
            )?;
        }

        let task_lock_path = task.source.mise_lock_path();
        let task_lock = if task_lock_path == "mise.lock" {
            None
        } else {
            optional_checked_source(&discovery.tool_checks, &task_lock_path)?
        };
        if let Some(task_lock_input) = task_lock {
            let NativeToolSource::MiseLock(task_lock) = &task_lock_input
                .native
                .as_ref()
                .ok_or_else(|| failure("build_task_source_mise_lock"))?
                .source
            else {
                return Err(failure("build_task_source_mise_lock"));
            };
            validate_lock_shape(task_lock)?;
            validate_source_lock_subset(task_lock, mise_lock)?;
        }
        let source_lock = if task_lock_path == "mise.lock" {
            Some(mise_lock)
        } else if let Some(task_lock_input) = task_lock {
            match &task_lock_input
                .native
                .as_ref()
                .ok_or_else(|| failure("build_task_source_mise_lock"))?
                .source
            {
                NativeToolSource::MiseLock(lock) => Some(lock),
                NativeToolSource::RustToolchain | NativeToolSource::MiseConfig(_) => {
                    return Err(failure("build_task_source_mise_lock"));
                }
            }
        } else {
            None
        };
        validate_source_task_lock_requests(
            &source_task_tools,
            source_lock,
            task_lock_path == "mise.lock",
            mise_config,
            mise_lock,
            rust_version,
            &rust_lock_options,
        )?;
        let task_lock_digest = task_lock.map(native_sha256).transpose()?;

        let task_rust_path = task.source.rust_toolchain_path();
        let task_rust_digest = if task_rust_path == "rust-toolchain.toml" {
            None
        } else {
            optional_checked_source(&discovery.tool_checks, &task_rust_path)?
                .map(native_sha256)
                .transpose()?
        };
        if task_rust_digest
            .as_deref()
            .is_some_and(|digest| digest != root_rust_sha256)
        {
            return Err(failure(
                "build_task_source_rust_toolchain_differs_from_root",
            ));
        }

        source_digests.push(BuildTaskSourceDigests {
            mise_config: native_sha256(task_config_input)?,
            mise_lock: task_lock_digest,
            rust_toolchain: task_rust_digest,
            selected_tools,
        });
    }

    build_tasks
        .iter()
        .zip(source_digests)
        .map(|(task, source)| {
            validate_mbx_tool_closure(task, &source.selected_tools)?;
            Ok(BuildTaskPolicy {
                task: (*task).clone(),
                runner_label: task.runner.runs_on().to_owned(),
                mise_setup: resolve_build_task_mise_setup(config, task.runner)?,
                mise_config_sha256: native_sha256(mise)?,
                mise_lock_sha256: root_lock_sha256.clone(),
                rust_toolchain_sha256: root_rust_sha256.clone(),
                source_mise_config_sha256: source.mise_config,
                source_mise_lock_sha256: source.mise_lock,
                source_rust_toolchain_sha256: source.rust_toolchain,
                selected_tools: source.selected_tools,
            })
        })
        .collect()
}

struct BuildTaskSourceDigests {
    mise_config: String,
    mise_lock: Option<String>,
    rust_toolchain: Option<String>,
    selected_tools: Vec<BuildTaskTool>,
}

fn checked_source<'a>(
    checks: &'a [ToolInputCheck],
    path: &str,
) -> Result<&'a ToolInputCheck, OrchestratorError> {
    checks
        .iter()
        .find(|check| check.path == path)
        .filter(|check| check.present && check.parse == ToolParse::Valid)
        .and_then(|check| check.native.as_ref().map(|_| check))
        .ok_or_else(|| failure("build_task_source_missing_or_invalid"))
}

fn optional_checked_source<'a>(
    checks: &'a [ToolInputCheck],
    path: &str,
) -> Result<Option<&'a ToolInputCheck>, OrchestratorError> {
    let check = checks
        .iter()
        .find(|check| check.path == path)
        .ok_or_else(|| failure("build_task_source_missing_or_invalid"))?;
    if !check.present && check.parse == ToolParse::Missing {
        return Ok(None);
    }
    check
        .present
        .then_some(check)
        .filter(|check| check.parse == ToolParse::Valid && check.native.is_some())
        .map(Some)
        .ok_or_else(|| failure("build_task_source_missing_or_invalid"))
}

/// SHA-256 identity of a checked source's native projection.
fn native_sha256(check: &ToolInputCheck) -> Result<String, OrchestratorError> {
    check
        .native
        .as_ref()
        .map(|input| input.sha256.clone())
        .ok_or_else(|| failure("build_task_source_missing_or_invalid"))
}

fn validate_config_shape(
    config: &NativeMiseConfig,
    rust_toolchain_version: &str,
) -> Result<(), OrchestratorError> {
    let allowed = ["min_version", "settings", "tasks", "tools"];
    if config
        .root_keys
        .iter()
        .any(|key| !allowed.contains(&key.as_str()))
        || !["settings", "tasks", "tools"]
            .iter()
            .all(|key| config.root_keys.iter().any(|actual| actual == key))
    {
        return Err(failure("build_task_mise_config_root"));
    }
    if !config.min_version_supported() {
        return Err(failure("build_task_mise_min_version"));
    }
    if rust_toolchain_version != RUST_VERSION {
        return Err(failure("build_task_rust_toolchain_version"));
    }
    let rust = config
        .tools
        .get("rust")
        .ok_or_else(|| failure("build_task_rust_selector"))?;
    if !rust.valid_shape
        || !rust.unsupported_options.is_empty()
        || rust.version.as_deref() != Some(rust_toolchain_version)
        || rust.os.is_some()
        || !rust.config_options.is_empty()
        || rust.mr_boxington != Some(true)
    {
        return Err(failure("build_task_rust_selector"));
    }
    let mbx = config
        .tools
        .get("mr-boxington")
        .ok_or_else(|| failure("build_task_mbx_selector"))?;
    if !mbx.valid_shape
        || !mbx.unsupported_options.is_empty()
        || mbx.version.as_deref() != Some(MR_BOXINGTON_VERSION)
        || mbx.os.is_some()
        || !mbx.config_options.is_empty()
        || mbx.mr_boxington.is_some()
    {
        return Err(failure("build_task_mbx_selector"));
    }
    let settings = &config.settings;
    if !settings.present
        || !settings.valid_shape
        || settings.lockfile != Some(true)
        || settings
            .idiomatic_version_file_enable_tools
            .as_deref()
            .is_none_or(|tools| tools.len() != 1 || tools[0] != "rust")
        || settings.cargo_binstall != Some(true)
        || settings.cargo_binstall_only == Some(false)
        || !settings.unsupported_fields.is_empty()
    {
        return Err(failure("build_task_mise_settings"));
    }
    Ok(())
}

/// Rust build tasks must select the separately pinned MBX executable.
fn validate_mbx_tool_closure(
    task: &velnor_actions_contract::BuildTask,
    selected_tools: &[velnor_actions_workflow_renderer::verification_jobs::BuildTaskTool],
) -> Result<(), OrchestratorError> {
    let uses_rust = task.tools.iter().any(|tool| tool == "rust");
    let selects_mbx = selected_tools.iter().any(|tool| tool.key == "mr-boxington");
    if uses_rust && !selects_mbx {
        return Err(failure("build_task_rust_requires_mbx"));
    }
    Ok(())
}

fn validate_task_tool_selection(
    task: &BuildTask,
    requested: &BTreeMap<String, String>,
    selected_tools: &[BuildTaskTool],
) -> Result<(), OrchestratorError> {
    for (key, version) in requested {
        if !task.tools.iter().any(|declared| declared == key) {
            return Err(failure("build_task_task_tool_not_declared"));
        }
        let selected = selected_tools
            .iter()
            .find(|tool| tool.key == *key)
            .ok_or_else(|| failure("build_task_task_tool_not_selected"))?;
        if selected.version != *version {
            return Err(failure("build_task_task_tool_version"));
        }
    }
    Ok(())
}

fn validate_task_source_shape(
    path: &str,
    config: &NativeMiseConfig,
) -> Result<(), OrchestratorError> {
    if path == "mise.toml" {
        return Ok(());
    }
    if config.root_keys != ["tasks"]
        || config.tasks.is_empty()
        || !config.tools.is_empty()
        || config.settings.present
    {
        return Err(failure("build_task_source_config_must_only_declare_tasks"));
    }
    Ok(())
}

fn exact_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn failure(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}

fn build_task_graph_problem(problem: &str) -> &'static str {
    match problem {
        "verification_task_graph" => "build_task_task_graph",
        "verification_task_graph_bound" => "build_task_task_graph_bound",
        "verification_task_missing" => "build_task_task_missing",
        "verification_task_shape" => "build_task_task_shape",
        "verification_task_run_shape" => "build_task_task_run_shape",
        "verification_task_tool_shape" => "build_task_task_tool_shape",
        "verification_task_tool_conflict" => "build_task_task_tool_conflict",
        "verification_task_dependency" => "build_task_task_dependency",
        "verification_task_nested_call" => "build_task_task_nested_call",
        "build_task_task_empty" => "build_task_task_empty",
        _ => "build_task_task_graph",
    }
}

#[cfg(test)]
#[path = "build_tasks_tests.rs"]
mod tests;
