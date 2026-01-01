//! Selected native tool resolution against source-bound Mise rows.

use std::collections::BTreeMap;

use velnor_actions_contract::BuildTask;
use velnor_actions_workflow_renderer::verification_jobs::{BuildTaskArtifact, BuildTaskTool};

use crate::OrchestratorError;
use crate::native_tool_input::NativeMiseConfig;
use crate::native_tool_lock::{NativeLockedTool, NativeMiseLock};

/// (`version`, `options`, `extra_args`) selected for a tool.
pub(crate) type ToolVersionSelection = (String, BTreeMap<String, String>, Vec<String>);

const BOLTFFI_KEY: &str = "github:boltffi/boltffi";
const BOLTFFI_MATCHING_REGEX: &str = r"^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$";

pub(crate) fn resolve_selected_tools(
    task: &BuildTask,
    config: &NativeMiseConfig,
    lock: &NativeMiseLock,
    rust_version: &str,
    rust_lock_options: &BTreeMap<String, String>,
) -> Result<Vec<BuildTaskTool>, OrchestratorError> {
    task.tools
        .iter()
        .map(|key| {
            let (version, config_options, os) =
                selected_version_and_options(key, config, rust_version, rust_lock_options)?;
            let selected = lock
                .selected_tool(key, &version, &version, &config_options)
                .ok_or_else(|| {
                    failure(if lock.tools.contains_key(key) {
                        "build_task_tool_lock_mismatch"
                    } else {
                        "build_task_tool_unlocked"
                    })
                })?;
            if !selected.valid_shape
                || !selected.unsupported_fields.is_empty()
                || selected.version.as_deref() != Some(version.as_str())
                || !selected.specifiers.as_ref().is_some_and(|specifiers| {
                    specifiers.iter().any(|specifier| specifier == &version)
                })
                || (key == "rust" && &selected.options != rust_lock_options)
            {
                return Err(failure("build_task_tool_lock_mismatch"));
            }
            let backend = selected
                .backend
                .clone()
                .ok_or_else(|| failure("build_task_tool_backend"))?;
            let artifact = selected_artifact(key, selected)?;
            validate_backend_selection(key, &backend, &config_options, selected)?;
            Ok(BuildTaskTool {
                key: (*key).clone(),
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

pub(crate) fn selected_version_and_options(
    key: &str,
    config: &NativeMiseConfig,
    rust_version: &str,
    rust_lock_options: &BTreeMap<String, String>,
) -> Result<ToolVersionSelection, OrchestratorError> {
    if key == "rust" {
        return Ok((
            rust_version.to_owned(),
            rust_lock_options.clone(),
            Vec::new(),
        ));
    }
    let selector = config
        .tools
        .get(key)
        .ok_or_else(|| failure("build_task_tool_unconfigured"))?;
    if !selector.valid_shape
        || !selector.unsupported_options.is_empty()
        || selector.mr_boxington.is_some()
    {
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
    Ok((version, selector.config_options.clone(), os))
}

fn selected_artifact(
    key: &str,
    selected: &NativeLockedTool,
) -> Result<Option<BuildTaskArtifact>, OrchestratorError> {
    if key == "rust" {
        if !selected.platforms.is_empty() {
            return Err(failure("build_task_rust_lock_artifact"));
        }
        return Ok(None);
    }
    let artifact = selected
        .platforms
        .get("macos-arm64")
        .ok_or_else(|| failure("build_task_tool_artifact"))?;
    if !artifact.valid_shape
        || !artifact.unsupported_fields.is_empty()
        || artifact.checksum.is_none()
        || artifact.url.is_none()
    {
        return Err(failure("build_task_tool_artifact"));
    }
    Ok(Some(BuildTaskArtifact {
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
    }))
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

fn failure(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}
