//! Resolve native build tasks against the exact checked-in Mise source.

use std::collections::BTreeSet;

use velnor_actions_contract::{VelnorConfig, WorkflowTask, is_valid_mise_task_name};
use velnor_actions_workflow_renderer::{BuildTaskArtifact, BuildTaskPolicy, BuildTaskTool};

use crate::native_tool_input::{
    NativeMiseConfig, NativeMiseTask, NativeToolInput, NativeToolSource,
};
use crate::native_tool_lock::{NativeLockedTool, NativeMiseLock, rust_toolchain_options};
use crate::pins::resolve_build_task_mise_setup;
use crate::toolcheck::{ToolInputCheck, ToolParse};
use crate::{OrchestratorError, discover::Discovery};

const BOLTFFI_KEY: &str = "github:boltffi/boltffi";
const BOLTFFI_MATCHING_REGEX: &str = r"^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$";
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
    for task in &build_tasks {
        task.validate(".velnor/config.toml")
            .map_err(|_| failure("build_task_contract"))?;
        validate_task_closure(mise_config, &task.mise_task)?;
    }

    build_tasks
        .iter()
        .map(|task| {
            let selected_tools = resolve_selected_tools(
                task,
                mise_config,
                mise_lock,
                rust_version,
                &rust_lock_options,
            )?;
            Ok(BuildTaskPolicy {
                task: task.clone(),
                runner_label: task.runner.runs_on().to_owned(),
                mise_setup: resolve_build_task_mise_setup(config, task.runner)?,
                mise_config_sha256: mise.sha256.clone(),
                mise_lock_sha256: lock.sha256.clone(),
                rust_toolchain_sha256: rust.sha256.clone(),
                selected_tools,
            })
        })
        .collect()
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

fn validate_config_shape(config: &NativeMiseConfig) -> Result<(), OrchestratorError> {
    let allowed = ["settings", "tasks", "tools", "wrappers"];
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
    if !lock.valid_shape || lock.root_keys.len() != 1 || lock.root_keys[0] != "tools" {
        return Err(failure("build_task_mise_lock_root"));
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

fn resolve_selected_tools(
    task: &BuildTask,
    config: &NativeMiseConfig,
    lock: &NativeMiseLock,
    rust_version: &str,
    rust_lock_options: &BTreeMap<String, String>,
) -> Result<Vec<BuildTaskTool>, OrchestratorError> {
    if config.tools.contains_key("rust") {
        return Err(failure("build_task_rust_must_use_idiomatic_file"));
    }
    task.tools
        .iter()
        .map(|key| {
            let selected = lock
                .tools
                .get(key)
                .ok_or_else(|| failure("build_task_tool_unlocked"))?;
            let (version, config_options, os) = if key == "rust" {
                (
                    rust_version.to_owned(),
                    rust_lock_options.clone(),
                    Vec::new(),
                )
            } else {
                let selector = config
                    .tools
                    .get(key)
                    .ok_or_else(|| failure("build_task_tool_unconfigured"))?;
                if !selector.valid_shape || !selector.unsupported_options.is_empty() {
                    return Err(failure("build_task_tool_selector"));
                }
                let version = selector
                    .version
                    .clone()
                    .ok_or_else(|| failure("build_task_tool_selector"))?;
                let os = selector.os.clone().unwrap_or_default();
                if selector.os.is_some() && os != ["macos"] {
                    return Err(failure("build_task_tool_selector_os"));
                }
                (version, selector.config_options.clone(), os)
            };
            if !selected.valid_shape
                || !selected.unsupported_fields.is_empty()
                || selected.version.as_deref() != Some(version.as_str())
                || (key == "rust" && &selected.options != rust_lock_options)
            {
                return Err(failure("build_task_tool_lock_mismatch"));
            }
            let backend = selected
                .backend
                .clone()
                .ok_or_else(|| failure("build_task_tool_backend"))?;
            let artifact = if key == "rust" {
                if selected.macos_arm64.is_some() {
                    return Err(failure("build_task_rust_lock_artifact"));
                }
                None
            } else {
                let artifact = selected
                    .macos_arm64
                    .as_ref()
                    .ok_or_else(|| failure("build_task_tool_artifact"))?;
                if !artifact.valid_shape
                    || !artifact.unsupported_fields.is_empty()
                    || artifact.checksum.is_none()
                    || artifact.url.is_none()
                {
                    return Err(failure("build_task_tool_artifact"));
                }
                Some(BuildTaskArtifact {
                    checksum: artifact
                        .checksum
                        .clone()
                        .ok_or_else(|| failure("build_task_tool_artifact"))?,
                    url: artifact
                        .url
                        .clone()
                        .ok_or_else(|| failure("build_task_tool_artifact"))?,
                    url_api: artifact.url_api.clone(),
                    signer: artifact.signer.clone(),
                    provenance: artifact.provenance.clone(),
                })
            };
            validate_backend_selection(key, &backend, &config_options, selected)?;
            Ok(BuildTaskTool {
                key: key.clone(),
                version,
                backend,
                os,
                config_options,
                lock_options: selected.options.clone(),
                artifact,
            })
        })
        .collect()
}

fn validate_backend_selection(
    key: &str,
    backend: &str,
    config_options: &BTreeMap<String, String>,
    locked: &NativeLockedTool,
) -> Result<(), OrchestratorError> {
    let expected_boltffi = BTreeMap::from([(
        "matching_regex".to_owned(),
        BOLTFFI_MATCHING_REGEX.to_owned(),
    )]);
    match (key, backend) {
        ("rust", "core:rust")
            if locked
                .options
                .keys()
                .all(|key| matches!(key.as_str(), "components" | "targets")) => {}
        ("mr-boxington", "packslip:github.com/jdx/mr-boxington")
            if config_options.is_empty() && locked.options.is_empty() => {}
        (BOLTFFI_KEY, BOLTFFI_KEY)
            if config_options == &expected_boltffi && locked.options == expected_boltffi => {}
        (_, backend)
            if backend.starts_with("aqua:")
                && config_options.is_empty()
                && locked.options.is_empty() => {}
        (key, backend)
            if key.starts_with("github:")
                && key == backend
                && config_options.is_empty()
                && locked.options.is_empty() => {}
        _ => return Err(failure("build_task_tool_backend_unsupported")),
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
