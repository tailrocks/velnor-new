//! Resolve native build tasks against the exact checked-in Mise source.

use std::collections::BTreeMap;

use velnor_actions_contract::BuildTask;
use velnor_actions_mise::catalog::{MR_BOXINGTON_VERSION, RUST_VERSION};
use velnor_actions_workflow_renderer::verification_jobs::BuildTaskTool;

use crate::OrchestratorError;
use crate::native_tool_input::NativeMiseConfig;
use crate::toolcheck::{ToolInputCheck, ToolParse};
#[path = "build_task_lock_validation.rs"]
mod build_task_lock_validation;
pub(super) use build_task_lock_validation::{
    source_task_tool_requests, validate_source_lock_subset, validate_source_task_lock_requests,
};

#[path = "build_task_policy.rs"]
mod source_policy;
pub(crate) use source_policy::policies;

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
        "verification_task_graph" | "build_task_task_graph" => "build_task_task_graph",
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
