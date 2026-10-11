use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{BuildTask, VelnorConfig, WorkflowTask};
use velnor_actions_workflow_renderer::verification_jobs::{BuildTaskPolicy, BuildTaskTool};

use super::build_task_lock_validation::{
    source_task_tool_requests, validate_lock_shape, validate_source_lock_subset,
    validate_source_task_lock_requests,
};
use super::{
    build_task_graph_problem, checked_source, exact_version, failure, native_sha256,
    optional_checked_source, validate_config_shape, validate_mbx_tool_closure,
    validate_task_source_shape, validate_task_tool_selection,
};
use crate::build_task_tools::resolve_selected_tools;
use crate::native_tool_input::{NativeMiseConfig, NativeToolSource};
use crate::native_tool_lock::{NativeMiseLock, rust_toolchain_options};
use crate::pins::resolve_build_task_mise_setup;
use crate::toolcheck::ToolInputCheck;
use crate::{OrchestratorError, discover::Discovery};

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

    let root = root_build_task_sources(discovery)?;
    let source_digests = build_tasks
        .iter()
        .map(|task| source_task_digests(task, discovery, &root))
        .collect::<Result<Vec<_>, _>>()?;
    build_tasks
        .iter()
        .zip(source_digests)
        .map(|(task, source)| {
            validate_mbx_tool_closure(task, &source.selected_tools)?;
            Ok(BuildTaskPolicy {
                task: (*task).clone(),
                runner_label: task.runner.runs_on().to_owned(),
                mise_setup: resolve_build_task_mise_setup(config, task.runner)?,
                mise_config_sha256: native_sha256(root.mise_check)?,
                mise_lock_sha256: root.lock_sha256.clone(),
                rust_toolchain_sha256: root.rust_sha256.clone(),
                source_mise_config_sha256: source.mise_config,
                source_mise_lock_sha256: source.mise_lock,
                source_rust_toolchain_sha256: source.rust_toolchain,
                selected_tools: source.selected_tools,
            })
        })
        .collect()
}

struct RootBuildTaskSources<'a> {
    rust_version: &'a str,
    rust_lock_options: BTreeMap<String, String>,
    mise_check: &'a ToolInputCheck,
    mise_config: &'a NativeMiseConfig,
    mise_lock: &'a NativeMiseLock,
    lock_sha256: String,
    rust_sha256: String,
}

fn root_build_task_sources(
    discovery: &Discovery,
) -> Result<RootBuildTaskSources<'_>, OrchestratorError> {
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

    let mise_check = checked_source(&discovery.tool_checks, "mise.toml")?;
    let NativeToolSource::MiseConfig(mise_config) = &mise_check
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
    Ok(RootBuildTaskSources {
        rust_version,
        rust_lock_options,
        mise_check,
        mise_config,
        mise_lock,
        lock_sha256: native_sha256(lock)?,
        rust_sha256: native_sha256(rust)?,
    })
}

fn source_task_digests(
    task: &BuildTask,
    discovery: &Discovery,
    root: &RootBuildTaskSources<'_>,
) -> Result<BuildTaskSourceDigests, OrchestratorError> {
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
    let tool_selection = selected_task_tools(task, task_config, root)?;
    let task_lock =
        source_task_lock_digest(task, discovery, root, &tool_selection.source_requests)?;
    let task_rust = source_task_rust_digest(task, discovery, &root.rust_sha256)?;
    Ok(BuildTaskSourceDigests {
        mise_config: native_sha256(task_config_input)?,
        mise_lock: task_lock,
        rust_toolchain: task_rust,
        selected_tools: tool_selection.selected,
    })
}

struct TaskToolSelection {
    selected: Vec<BuildTaskTool>,
    source_requests: BTreeSet<(String, String)>,
}

fn selected_task_tools(
    task: &BuildTask,
    task_config: &NativeMiseConfig,
    root: &RootBuildTaskSources<'_>,
) -> Result<TaskToolSelection, OrchestratorError> {
    let mut merged_tasks = root.mise_config.clone();
    merged_tasks.tasks.extend(task_config.tasks.clone());
    let local_task_tools =
        crate::native_mise_tasks::selected_build_task_tools(&merged_tasks, &task.mise_task)
            .map_err(|problem| failure(build_task_graph_problem(problem)))?;
    let selected_tools = resolve_selected_tools(
        task,
        root.mise_config,
        root.mise_lock,
        root.rust_version,
        &root.rust_lock_options,
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
    Ok(TaskToolSelection {
        selected: selected_tools,
        source_requests: source_task_tools,
    })
}

fn source_task_lock_digest(
    task: &BuildTask,
    discovery: &Discovery,
    root: &RootBuildTaskSources<'_>,
    source_task_tools: &BTreeSet<(String, String)>,
) -> Result<Option<String>, OrchestratorError> {
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
        validate_source_lock_subset(task_lock, root.mise_lock)?;
    }
    let source_lock = if task_lock_path == "mise.lock" {
        Some(root.mise_lock)
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
        source_task_tools,
        source_lock,
        task_lock_path == "mise.lock",
        root.mise_config,
        root.mise_lock,
        root.rust_version,
        &root.rust_lock_options,
    )?;
    task_lock.map(native_sha256).transpose()
}

fn source_task_rust_digest(
    task: &BuildTask,
    discovery: &Discovery,
    root_rust_sha256: &str,
) -> Result<Option<String>, OrchestratorError> {
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
    Ok(task_rust_digest)
}

struct BuildTaskSourceDigests {
    mise_config: String,
    mise_lock: Option<String>,
    rust_toolchain: Option<String>,
    selected_tools: Vec<BuildTaskTool>,
}
