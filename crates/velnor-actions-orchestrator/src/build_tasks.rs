//! Resolve native build tasks against the exact checked-in Mise source.

use std::collections::BTreeSet;

use velnor_actions_contract::{VelnorConfig, WorkflowTask, is_valid_mise_task_name};
use velnor_actions_workflow_renderer::verification_jobs::BuildTaskPolicy;

use crate::build_task_tools::resolve_selected_tools;
use crate::native_tool_input::{NativeMiseConfig, NativeMiseTask, NativeToolSource};
use crate::native_tool_lock::{NativeMiseLock, rust_toolchain_options};
use crate::pins::resolve_build_task_mise_setup;
use crate::toolcheck::{ToolInputCheck, ToolParse};
use crate::{OrchestratorError, discover::Discovery};

const MAX_TASK_CLOSURE: usize = 128;

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

    validate_config_shape(mise_config)?;
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
        validate_task_closure(&merged_tasks, &task.mise_task)?;

        let task_lock_path = task.source.mise_lock_path();
        let task_lock_digest = if task_lock_path == "mise.lock" {
            None
        } else {
            optional_checked_source(&discovery.tool_checks, &task_lock_path)?
                .map(native_sha256)
                .transpose()?
        };
        if task_lock_digest
            .as_deref()
            .is_some_and(|digest| digest != root_lock_sha256)
        {
            return Err(failure("build_task_source_lock_differs_from_root"));
        }

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
        });
    }

    build_tasks
        .iter()
        .zip(source_digests)
        .map(|(task, source)| {
            let selected_tools = resolve_selected_tools(
                task,
                mise_config,
                mise_lock,
                rust_version,
                &rust_lock_options,
            )?;
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
                selected_tools,
            })
        })
        .collect()
}

struct BuildTaskSourceDigests {
    mise_config: String,
    mise_lock: Option<String>,
    rust_toolchain: Option<String>,
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

fn validate_config_shape(config: &NativeMiseConfig) -> Result<(), OrchestratorError> {
    let allowed = ["min_version", "settings", "tasks", "tools", "wrappers"];
    if config
        .root_keys
        .iter()
        .any(|key| !allowed.contains(&key.as_str()))
        || !["settings", "tasks", "tools", "wrappers"]
            .iter()
            .all(|key| config.root_keys.iter().any(|actual| actual == key))
    {
        return Err(failure("build_task_mise_config_root"));
    }
    if !config.min_version_supported() {
        return Err(failure("build_task_mise_min_version"));
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
    let wrappers = &config.wrappers;
    if !wrappers.present
        || !wrappers.valid_shape
        || wrappers.cargo_command.as_deref() != Some("mbx")
        || wrappers.mbx_cargo_shim_mode.as_deref() != Some("1")
        || !wrappers.unsupported_fields.is_empty()
    {
        return Err(failure("build_task_mise_mbx_wrapper"));
    }
    Ok(())
}

fn validate_lock_shape(lock: &NativeMiseLock) -> Result<(), OrchestratorError> {
    if !lock.has_supported_root_shape() {
        return Err(failure("build_task_mise_lock_root"));
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
        || config.wrappers.present
    {
        return Err(failure("build_task_source_config_must_only_declare_tasks"));
    }
    Ok(())
}

fn validate_task_closure(
    config: &NativeMiseConfig,
    task_name: &str,
) -> Result<(), OrchestratorError> {
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut count = 0;
    visit_task(config, task_name, &mut active, &mut complete, &mut count)
}

fn visit_task(
    config: &NativeMiseConfig,
    name: &str,
    active: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
    count: &mut usize,
) -> Result<(), OrchestratorError> {
    if complete.contains(name) {
        return Ok(());
    }
    if !is_valid_mise_task_name(name) || !active.insert(name.to_owned()) {
        return Err(failure("build_task_task_graph"));
    }
    *count += 1;
    if *count > MAX_TASK_CLOSURE {
        return Err(failure("build_task_task_graph_bound"));
    }
    let task = config
        .tasks
        .get(name)
        .ok_or_else(|| failure("build_task_task_missing"))?;
    let children = task_children(task)?;
    for child in children {
        visit_task(config, &child, active, complete, count)?;
    }
    active.remove(name);
    complete.insert(name.to_owned());
    Ok(())
}

fn task_children(task: &NativeMiseTask) -> Result<Vec<String>, OrchestratorError> {
    if !task.valid_shape || !task.unsupported_fields.is_empty() || !task.task_tools.is_empty() {
        return Err(failure("build_task_task_shape"));
    }
    if task.run_field_present && task.run_commands.is_none() {
        return Err(failure("build_task_task_run_shape"));
    }
    let mut children = BTreeSet::new();
    for dependency in &task.dependencies {
        if !is_valid_mise_task_name(dependency) {
            return Err(failure("build_task_task_dependency"));
        }
        children.insert(dependency.clone());
    }
    for command in task.run_commands.iter().flatten() {
        for line in command.lines() {
            let line = line.trim();
            if !line.contains("mise") {
                continue;
            }
            let words = line.split_whitespace().collect::<Vec<_>>();
            if words.len() != 3
                || words[0] != "mise"
                || words[1] != "run"
                || !is_valid_mise_task_name(words[2])
            {
                return Err(failure("build_task_task_nested_call"));
            }
            children.insert(words[2].to_owned());
        }
    }
    if !task.run_field_present && children.is_empty() {
        return Err(failure("build_task_task_empty"));
    }
    if children.len() > MAX_TASK_CLOSURE {
        return Err(failure("build_task_task_graph_bound"));
    }
    Ok(children.into_iter().collect())
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
