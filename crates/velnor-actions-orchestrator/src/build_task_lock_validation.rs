//! Strict validation of root and task-local native Mise lock rows.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::is_valid_build_tool_key;

use crate::OrchestratorError;
use crate::build_task_tools::selected_version_and_options;
use crate::native_mise_tasks::safe_task_tool_version;
use crate::native_tool_input::NativeMiseConfig;
use crate::native_tool_lock::NativeMiseLock;

use super::failure;

pub(super) fn validate_lock_shape(lock: &NativeMiseLock) -> Result<(), OrchestratorError> {
    if !lock.has_supported_root_shape() {
        return Err(failure("build_task_mise_lock_root"));
    }
    Ok(())
}

/// A task-local lock may narrow the root lock, but it cannot introduce or
/// alter any artifact row. Its raw source digest remains separately bound.
pub(super) fn validate_source_lock_subset(
    source: &NativeMiseLock,
    root: &NativeMiseLock,
) -> Result<(), OrchestratorError> {
    if !source.has_supported_root_shape() || !root.has_supported_root_shape() {
        return Err(failure("build_task_source_mise_lock"));
    }
    for (key, rows) in &source.tools {
        let root_rows = root
            .tools
            .get(key)
            .ok_or_else(|| failure("build_task_source_lock_foreign_tool"))?;
        if rows.is_empty() {
            return Err(failure("build_task_source_lock_empty_rows"));
        }
        for (index, row) in rows.iter().enumerate() {
            if !supported_lock_row(row) || rows[..index].contains(row) {
                return Err(failure("build_task_source_lock_row_shape"));
            }
            if root_rows
                .iter()
                .filter(|root_row| {
                    supported_lock_row(root_row) && lock_row_is_subset(row, root_row)
                })
                .count()
                != 1
            {
                return Err(failure("build_task_source_lock_not_root_subset"));
            }
        }
    }
    Ok(())
}

fn lock_row_is_subset(
    source: &crate::native_tool_lock::NativeLockedTool,
    root: &crate::native_tool_lock::NativeLockedTool,
) -> bool {
    source.version == root.version
        && source.backend == root.backend
        && source.specifiers == root.specifiers
        && source.options == root.options
        && source
            .platforms
            .iter()
            .all(|(platform, artifact)| root.platforms.get(platform) == Some(artifact))
}

fn supported_lock_row(row: &crate::native_tool_lock::NativeLockedTool) -> bool {
    row.valid_shape
        && row.unsupported_fields.is_empty()
        && row
            .version
            .as_deref()
            .is_some_and(|version| !version.is_empty())
        && row
            .backend
            .as_deref()
            .is_some_and(|backend| !backend.is_empty())
        && row
            .specifiers
            .as_ref()
            .is_some_and(|specifiers| !specifiers.is_empty())
        && row.platforms.values().all(|artifact| {
            artifact.valid_shape
                && artifact.unsupported_fields.is_empty()
                && artifact
                    .checksum
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                && artifact
                    .url
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
        })
}

pub(super) fn source_task_tool_requests(
    config: &NativeMiseConfig,
) -> Result<BTreeSet<(String, String)>, OrchestratorError> {
    let mut requests = BTreeSet::new();
    for task in config.tasks.values() {
        if task
            .unsupported_fields
            .iter()
            .any(|field| field == "tools.shape")
        {
            return Err(failure("build_task_task_tool_shape"));
        }
        for (key, version) in &task.task_tools {
            if !is_valid_build_tool_key(key) || !safe_task_tool_version(version) {
                return Err(failure("build_task_task_tool_shape"));
            }
            requests.insert((key.clone(), version.clone()));
        }
    }
    Ok(requests)
}

/// Bind every task-local selector to the root config and lock. A separate
/// task lock must contain exactly the local requests from its source config;
/// root-provided tool rows remain in the root lock.
pub(super) fn validate_source_task_lock_requests(
    requests: &BTreeSet<(String, String)>,
    source_lock: Option<&NativeMiseLock>,
    source_lock_is_root: bool,
    root_config: &NativeMiseConfig,
    root_lock: &NativeMiseLock,
    rust_version: &str,
    rust_options: &BTreeMap<String, String>,
) -> Result<(), OrchestratorError> {
    let Some(source_lock) = source_lock else {
        return if requests.is_empty() {
            Ok(())
        } else {
            Err(failure("build_task_source_task_lock_missing"))
        };
    };

    if !source_lock_is_root {
        let mut expected = BTreeMap::<&str, BTreeSet<&str>>::new();
        for (key, version) in requests {
            expected
                .entry(key.as_str())
                .or_default()
                .insert(version.as_str());
        }
        if source_lock.tools.len() != expected.len() {
            return Err(failure("build_task_source_task_lock_requests"));
        }
        for (key, rows) in &source_lock.tools {
            let versions = expected
                .get(key.as_str())
                .ok_or_else(|| failure("build_task_source_task_lock_requests"))?;
            if rows.len() != versions.len()
                || rows.iter().any(|row| {
                    row.version
                        .as_deref()
                        .is_none_or(|version| !versions.contains(version))
                })
            {
                return Err(failure("build_task_source_task_lock_requests"));
            }
        }
    }

    for (key, requested_version) in requests {
        let (version, options, _) =
            selected_version_and_options(key, root_config, rust_version, rust_options)?;
        if version.as_str() != requested_version.as_str() {
            return Err(failure("build_task_source_task_tool_version"));
        }
        let root_row = root_lock
            .selected_tool(key, &version, requested_version, &options)
            .filter(|row| supported_lock_row(row))
            .ok_or_else(|| failure("build_task_source_task_tool_root_lock"))?;
        let source_row = source_lock
            .selected_tool(key, &version, requested_version, &options)
            .filter(|row| supported_lock_row(row))
            .ok_or_else(|| failure("build_task_source_task_tool_source_lock"))?;
        if key != "rust" && !source_row.platforms.contains_key("macos-arm64") {
            return Err(failure("build_task_source_task_tool_platform_missing"));
        }
        if !lock_row_is_subset(source_row, root_row) {
            return Err(failure("build_task_source_task_tool_lock_mismatch"));
        }
    }
    Ok(())
}
