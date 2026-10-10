//! Construction of isolated verification-only task jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{VelnorConfig, VerificationRunner, WorkflowTask};
use velnor_actions_workflow_renderer::{
    VerificationTaskPolicy,
    verification_jobs::{BuildTaskArtifact, BuildTaskTool},
};

use crate::native_tool_input::{NativeMiseConfig, NativeToolInput, NativeToolSource};
use crate::native_tool_lock::{NativeLockedTool, NativeMiseLock, rust_toolchain_options};
use crate::pins::resolve_verification_mise_setup;
use crate::toolcheck::{ToolInputCheck, ToolParse};
use crate::{OrchestratorError, discover::Discovery};

/// (version, options) resolved for the rust toolchain.
type RustToolValues = (String, BTreeMap<String, String>);
/// (`version`, `options`, `extra_args`) selected for a tool.
type ToolVersionSelection = (String, BTreeMap<String, String>, Vec<String>);

/// Resolve each workflow task onto its fixed platform and Mise binary pin.
pub(crate) fn policies(
    config: &VelnorConfig,
    discovery: &Discovery,
) -> Result<Vec<VerificationTaskPolicy>, OrchestratorError> {
    let scale_set_token = match &config.execution {
        Some(execution) => Some(
            execution
                .scale_selector()
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })?
                .token(),
        ),
        None => None,
    };
    config
        .workflow
        .tasks
        .iter()
        .filter_map(|task| match task {
            WorkflowTask::Verification(verification) => Some(verification),
            WorkflowTask::Build(_) | WorkflowTask::NativeImage(_) => None,
        })
        .map(|task| {
            let resolved = resolve_verification_tools(&discovery.tool_checks, task)?;
            Ok(VerificationTaskPolicy {
                task: task.clone(),
                runner_label: task.runner.runs_on().to_owned(),
                scale_set_token: scale_set_token.clone(),
                mise_setup: resolve_verification_mise_setup(config, task.runner)?,
                selected_tools: resolved.selected_tools,
                mise_config_sha256: resolved.mise_config_sha256,
                mise_lock_sha256: resolved.mise_lock_sha256,
                rust_toolchain_sha256: resolved.rust_toolchain_sha256,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ResolvedVerificationTools {
    selected_tools: Vec<BuildTaskTool>,
    mise_config_sha256: Option<String>,
    mise_lock_sha256: Option<String>,
    rust_toolchain_sha256: Option<String>,
}

fn resolve_verification_tools(
    checks: &[ToolInputCheck],
    task: &velnor_actions_contract::VerificationTask,
) -> Result<ResolvedVerificationTools, OrchestratorError> {
    let mise_config_path = &task.source.mise_config;
    let mise_lock_path = task.source.mise_lock_path();
    let rust_toolchain_path = task.source.rust_toolchain_path();
    let mise = optional_source(checks, mise_config_path)?;
    let lock = optional_source(checks, &mise_lock_path)?;
    let rust = optional_source(checks, &rust_toolchain_path)?;
    let mise = mise.ok_or_else(|| failure("verification_mise_config_missing"))?;
    let mut result = ResolvedVerificationTools {
        mise_config_sha256: Some(mise.sha256.clone()),
        mise_lock_sha256: lock.map(|input| input.sha256.clone()),
        rust_toolchain_sha256: rust.map(|input| input.sha256.clone()),
        ..ResolvedVerificationTools::default()
    };
    let NativeToolSource::MiseConfig(mise_config) = &mise.source else {
        return Err(failure("verification_mise_config"));
    };
    validate_config_shape(mise_config)?;
    validate_settings(mise_config)?;
    validate_wrappers(mise_config)?;
    let requested = crate::native_mise_tasks::selected_task_tools(mise_config, &task.mise_task)
        .map_err(failure)?;
    if requested.is_empty() {
        if let Some(lock) = lock {
            let NativeToolSource::MiseLock(lock) = &lock.source else {
                return Err(failure("verification_mise_lock"));
            };
            validate_lock_shape(lock)?;
        }
        return Ok(result);
    }
    let lock = lock.ok_or_else(|| failure("verification_mise_lock_missing"))?;
    let NativeToolSource::MiseLock(mise_lock) = &lock.source else {
        return Err(failure("verification_mise_lock"));
    };
    validate_lock_shape(mise_lock)?;
    if requested.contains_key("rust") && mise_config.tools.contains_key("rust") {
        return Err(failure("verification_rust_must_use_idiomatic_file"));
    }
    let rust_values = rust_tool_values(checks, rust, &requested, &rust_toolchain_path)?;
    let (mise_os, lock_platform) = platform(task.runner);
    for (key, requested_version) in requested {
        let locked = mise_lock
            .tools
            .get(&key)
            .ok_or_else(|| failure("verification_tool_unlocked"))?;
        let (version, config_options, os) = requested_tool_version(
            &key,
            requested_version,
            mise_config,
            mise_os,
            rust_values.as_ref(),
            locked,
        )?;
        result.selected_tools.push(resolve_tool(
            &key,
            &version,
            config_options,
            os,
            locked,
            lock_platform,
        )?);
    }
    Ok(result)
}

fn rust_tool_values(
    checks: &[ToolInputCheck],
    rust: Option<&NativeToolInput>,
    requested: &BTreeMap<String, String>,
    rust_toolchain_path: &str,
) -> Result<Option<RustToolValues>, OrchestratorError> {
    if !requested.contains_key("rust") {
        return Ok(None);
    }
    let rust = rust.ok_or_else(|| failure("verification_rust_toolchain_missing"))?;
    let NativeToolSource::RustToolchain = &rust.source else {
        return Err(failure("verification_rust_toolchain"));
    };
    let values = checks
        .iter()
        .find(|check| check.path == rust_toolchain_path)
        .map(|check| &check.values)
        .ok_or_else(|| failure("verification_rust_toolchain_missing"))?;
    let version = values
        .get("channel")
        .filter(|value| exact_rust_version(value))
        .ok_or_else(|| failure("verification_rust_toolchain"))?;
    let options =
        rust_toolchain_options(values).ok_or_else(|| failure("verification_rust_options"))?;
    Ok(Some((version.to_owned(), options)))
}

/// True when (`key`, `backend`) names a pinned tool backend: rust core,
/// mr-boxington, boltffi, aqua, or a matching `github:` tool.
fn is_pinned_tool_backend(key: &str, backend: &str) -> bool {
    key == "rust" && backend == "core:rust"
        || key == "mr-boxington" && backend == "packslip:github.com/jdx/mr-boxington"
        || key == "github:boltffi/boltffi" && backend == key
        || backend.starts_with("aqua:") && backend.len() > "aqua:".len()
        || key.starts_with("github:") && key == backend
}

fn requested_tool_version(
    key: &str,
    requested_version: String,
    mise_config: &NativeMiseConfig,
    mise_os: &str,
    rust_values: Option<&RustToolValues>,
    locked: &NativeLockedTool,
) -> Result<ToolVersionSelection, OrchestratorError> {
    if key == "rust" {
        let (version, options) =
            rust_values.ok_or_else(|| failure("verification_rust_toolchain"))?;
        if requested_version != *version || locked.options != *options {
            return Err(failure("verification_rust_tool_lock"));
        }
        return Ok((version.clone(), options.clone(), Vec::new()));
    }
    match mise_config.tools.get(key) {
        Some(selector) => {
            if !selector.valid_shape || !selector.unsupported_options.is_empty() {
                return Err(failure("verification_tool_selector"));
            }
            if selector.version.as_deref() != Some(requested_version.as_str()) {
                return Err(failure("verification_tool_selector_version"));
            }
            let os = selector.os.clone().unwrap_or_default();
            if selector.os.is_some() && os != [mise_os] {
                return Err(failure("verification_tool_selector_os"));
            }
            Ok((requested_version, selector.config_options.clone(), os))
        }
        None => Ok((requested_version, BTreeMap::new(), Vec::new())),
    }
}

fn optional_source<'a>(
    checks: &'a [ToolInputCheck],
    path: &str,
) -> Result<Option<&'a NativeToolInput>, OrchestratorError> {
    let check = checks
        .iter()
        .find(|check| check.path == path)
        .ok_or_else(|| failure("verification_tool_input_missing"))?;
    match (&check.parse, check.present, check.native.as_ref()) {
        (ToolParse::Missing, false, None) => Ok(None),
        (ToolParse::Valid, true, Some(native)) => Ok(Some(native)),
        _ => Err(failure("verification_tool_input_invalid")),
    }
}

fn validate_config_shape(config: &NativeMiseConfig) -> Result<(), OrchestratorError> {
    let allowed = ["settings", "tasks", "tools", "wrappers"];
    if config
        .root_keys
        .iter()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(failure("verification_mise_config_root"));
    }
    Ok(())
}

fn validate_settings(config: &NativeMiseConfig) -> Result<(), OrchestratorError> {
    if !config.root_keys.iter().any(|key| key == "settings") {
        return Ok(());
    }
    let settings = &config.settings;
    if !settings.present
        || !settings.valid_shape
        || settings.lockfile != Some(true)
        || settings
            .idiomatic_version_file_enable_tools
            .as_deref()
            .is_some_and(|tools| tools.len() != 1 || tools[0] != "rust")
        || settings.cargo_binstall == Some(false)
        || settings.cargo_binstall_only == Some(false)
        || (settings.cargo_binstall_only == Some(true) && settings.cargo_binstall != Some(true))
        || !settings.unsupported_fields.is_empty()
    {
        return Err(failure("verification_mise_settings"));
    }
    Ok(())
}

fn validate_wrappers(config: &NativeMiseConfig) -> Result<(), OrchestratorError> {
    if !config.root_keys.iter().any(|key| key == "wrappers") {
        return Ok(());
    }
    let wrappers = &config.wrappers;
    if !wrappers.present
        || !wrappers.valid_shape
        || wrappers.cargo_command.as_deref() != Some("mbx")
        || wrappers.mbx_cargo_shim_mode.as_deref() != Some("1")
        || !wrappers.unsupported_fields.is_empty()
    {
        return Err(failure("verification_mise_wrappers"));
    }
    Ok(())
}

fn validate_lock_shape(lock: &NativeMiseLock) -> Result<(), OrchestratorError> {
    if !lock.valid_shape || lock.root_keys.len() != 1 || lock.root_keys[0] != "tools" {
        return Err(failure("verification_mise_lock_root"));
    }
    Ok(())
}

fn resolve_tool(
    key: &str,
    version: &str,
    config_options: BTreeMap<String, String>,
    os: Vec<String>,
    locked: &NativeLockedTool,
    lock_platform: &str,
) -> Result<BuildTaskTool, OrchestratorError> {
    if !safe_version(version)
        || matches!(
            version,
            "latest" | "system" | "ref" | "stable" | "nightly" | "lts"
        )
        || key.starts_with("cargo:")
        || !locked.valid_shape
        || !locked.unsupported_fields.is_empty()
        || locked.version.as_deref() != Some(version)
    {
        return Err(failure("verification_tool_identity_or_lock"));
    }
    let backend = locked
        .backend
        .clone()
        .ok_or_else(|| failure("verification_tool_backend"))?;
    if backend.starts_with("cargo:") || !is_pinned_tool_backend(key, &backend) {
        return Err(failure("verification_tool_backend_unsupported"));
    }
    let artifact = if key == "rust" {
        if backend != "core:rust"
            || locked.macos_arm64.is_some()
            || locked.linux_x64.is_some()
            || !locked
                .options
                .keys()
                .all(|option| matches!(option.as_str(), "components" | "targets"))
        {
            return Err(failure("verification_rust_tool_lock"));
        }
        None
    } else {
        let locked_artifact = match lock_platform {
            "linux-x64" => locked.linux_x64.as_ref(),
            "macos-arm64" => locked.macos_arm64.as_ref(),
            _ => None,
        }
        .ok_or_else(|| failure("verification_tool_platform_artifact"))?;
        if !locked_artifact.valid_shape || !locked_artifact.unsupported_fields.is_empty() {
            return Err(failure("verification_tool_platform_artifact"));
        }
        Some(BuildTaskArtifact {
            checksum: locked_artifact
                .checksum
                .clone()
                .ok_or_else(|| failure("verification_tool_platform_artifact"))?,
            url: locked_artifact
                .url
                .clone()
                .ok_or_else(|| failure("verification_tool_platform_artifact"))?,
            url_api: locked_artifact.url_api.clone(),
            signer: locked_artifact.signer.clone(),
            provenance: locked_artifact.provenance.clone(),
        })
    };
    Ok(BuildTaskTool {
        key: key.to_owned(),
        version: version.to_owned(),
        backend,
        os,
        config_options,
        lock_options: locked.options.clone(),
        artifact,
    })
}

fn platform(runner: VerificationRunner) -> (&'static str, &'static str) {
    match runner {
        VerificationRunner::LinuxX64 => ("linux", "linux-x64"),
        VerificationRunner::MacosArm64 => ("macos", "macos-arm64"),
    }
}

fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_' | b'-'))
}

fn exact_rust_version(version: &str) -> bool {
    let parts = version.split('.').collect::<Vec<_>>();
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

#[cfg(test)]
#[path = "../tests/impl_verification_task_bootstrap.rs"]
mod tests;
