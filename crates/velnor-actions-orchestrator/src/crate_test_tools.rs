// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::cachekey::{SelectedTestTool, SelectedTestToolSourceBinding};
use velnor_actions_contract::config::project_tool_refs::PROJECT_TOOL_LOCK_FILES;
use velnor_actions_contract::{CrateTestTool, ProposedTask};
use velnor_actions_mise::selected_lock::{SelectedHkLockRow, SelectedMiseLock, select_hk_lock_row};
use velnor_actions_mise::validate_concrete_exact_version;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::native_tool_input::{NativeToolSource, native_mise_source};
use crate::project_tool_sources::{is_mise_config, source_paths};
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

pub(crate) fn resolve_for_packages(
    root: &Path,
    runner_label: &str,
    requested: &BTreeMap<String, Vec<CrateTestTool>>,
    discovery: &Discovery,
) -> Result<BTreeMap<String, Vec<SelectedTestTool>>, OrchestratorError> {
    if requested.is_empty() {
        return Ok(BTreeMap::new());
    }

    for package_name in requested.keys() {
        validate_package_owner(package_name, discovery)?;
    }

    let selection = resolve_hk(root, runner_label)?;
    requested
        .iter()
        .map(|(package, tools)| {
            let selected = tools
                .iter()
                .map(|tool| match tool {
                    CrateTestTool::Hk => selection.clone(),
                })
                .collect();
            Ok((package.clone(), selected))
        })
        .collect::<Result<BTreeMap<_, _>, OrchestratorError>>()
}

pub(crate) fn is_supported_test_task(task: &ProposedTask) -> bool {
    task.stack_id == "rust"
        && matches!(task.task_kind.as_str(), "test" | "nextest" | "doctest")
        && !task.no_targets
}

fn validate_package_owner(
    package_name: &str,
    discovery: &Discovery,
) -> Result<(), OrchestratorError> {
    let mut owners = BTreeSet::new();
    for package in discovery
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.record.packages.iter())
        .filter(|package| package.in_workspace && package.name == package_name)
    {
        owners.insert((package.id.as_str(), package.manifest.as_str()));
    }
    if owners.len() != 1 {
        return Err(config_error(
            ".velnor/config.toml",
            format!("stacks.rust.test_tools.{package_name}"),
            if owners.is_empty() {
                "unknown_rust_package"
            } else {
                "ambiguous_rust_package"
            },
        ));
    }
    let (package_id, manifest) = owners.first().ok_or_else(|| {
        config_error(
            ".velnor/config.toml",
            "stacks.rust.test_tools",
            "package_owner_missing",
        )
    })?;
    if !discovery.proposals.iter().any(|task| {
        is_supported_test_task(task)
            && task.display_name == package_name
            && task.identity.unit_id == *package_id
            && task.identity.unit_path == *manifest
    }) {
        return Err(config_error(
            ".velnor/config.toml",
            format!("stacks.rust.test_tools.{package_name}"),
            "no_runnable_test_obligation",
        ));
    }
    Ok(())
}

fn resolve_hk(root: &Path, runner_label: &str) -> Result<SelectedTestTool, OrchestratorError> {
    let platform = platform_for_runner(runner_label)?;
    let mut source_bindings = Vec::new();
    let mut selected_version: Option<String> = None;
    for path in source_paths(root)?
        .into_iter()
        .filter(|path| is_mise_config(path))
    {
        let RepoRead::Text(text) = read_repo_file(root, &path, MAX_REPO_FILE_BYTES)? else {
            continue;
        };
        let value: toml::Value = toml::from_str(&text).map_err(|error| {
            config_error(&path, "tools.hk", &format!("invalid_mise_config:{error}"))
        })?;
        let input = native_mise_source("mise.toml", text.as_bytes(), &value)
            .ok_or_else(|| config_error(&path, "tools.hk", "invalid_mise_config_shape"))?;
        let NativeToolSource::MiseConfig(config) = input.source else {
            return Err(config_error(&path, "tools.hk", "invalid_mise_config_shape"));
        };
        let Some(selector) = config.tools.get("hk") else {
            continue;
        };
        if !selector.valid_shape
            || !selector.unsupported_options.is_empty()
            || selector.os.is_some()
            || selector.mr_boxington.is_some()
            || !selector.config_options.is_empty()
        {
            return Err(config_error(&path, "tools.hk", "unsupported_hk_selector"));
        }
        let version = selector
            .version
            .as_deref()
            .ok_or_else(|| config_error(&path, "tools.hk", "hk_selector_version_missing"))?;
        validate_concrete_exact_version("hk", version)
            .map_err(|_| config_error(&path, "tools.hk", "hk_version_not_exact"))?;
        if selected_version
            .as_deref()
            .is_some_and(|previous| previous != version)
        {
            return Err(config_error(
                &path,
                "tools.hk",
                "conflicting_project_hk_versions",
            ));
        }
        selected_version = Some(version.to_owned());
        source_bindings.push(SelectedTestToolSourceBinding {
            path,
            sha256: input.sha256,
        });
    }
    let version = selected_version
        .ok_or_else(|| config_error("mise.toml", "tools.hk", "hk_project_pin_missing"))?;

    let (lock_path, lock, lock_hash) = read_hk_lock(root)?;
    let row = select_hk_lock_row(&lock, &version, platform)
        .map_err(|problem| config_error(&lock_path, "tools.hk", problem))?;
    source_bindings.push(SelectedTestToolSourceBinding {
        path: lock_path,
        sha256: lock_hash,
    });
    source_bindings.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(selected_hk_identity(row, source_bindings))
}

fn read_hk_lock(root: &Path) -> Result<(String, SelectedMiseLock, String), OrchestratorError> {
    let mut present = Vec::new();
    for path in PROJECT_TOOL_LOCK_FILES {
        if let RepoRead::Text(text) = read_repo_file(root, path, MAX_REPO_FILE_BYTES)? {
            present.push((path.to_owned(), text));
        }
    }
    let [(path, text)] = present.as_slice() else {
        return Err(config_error(
            "mise.lock",
            "tools.hk",
            if present.is_empty() {
                "hk_lock_missing"
            } else {
                "ambiguous_project_mise_lock"
            },
        ));
    };
    let value: toml::Value = toml::from_str(text)
        .map_err(|error| config_error(path, "tools.hk", &format!("invalid_mise_lock:{error}")))?;
    let input = native_mise_source("mise.lock", text.as_bytes(), &value)
        .ok_or_else(|| config_error(path, "tools.hk", "invalid_mise_lock_shape"))?;
    let NativeToolSource::MiseLock(lock) = input.source else {
        return Err(config_error(path, "tools.hk", "invalid_mise_lock_shape"));
    };
    Ok((path.clone(), lock, input.sha256))
}

fn selected_hk_identity(
    row: SelectedHkLockRow,
    source_bindings: Vec<SelectedTestToolSourceBinding>,
) -> SelectedTestTool {
    SelectedTestTool {
        tool: CrateTestTool::Hk,
        version: row.version,
        backend: row.backend,
        platform: row.platform,
        options: BTreeMap::new(),
        checksum: row.checksum,
        url: row.url,
        signer: row.signer,
        source_bindings,
    }
}

fn platform_for_runner(label: &str) -> Result<&'static str, OrchestratorError> {
    match label {
        "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04" => Ok("linux-x64"),
        "ubuntu-22.04-arm" | "ubuntu-24.04-arm" | "ubuntu-26.04-arm" => Ok("linux-arm64"),
        _ => Err(config_error(
            ".velnor/config.toml",
            "workflow.runner_label",
            "hk_runner_platform_unsupported",
        )),
    }
}

fn config_error(file: &str, field: impl Into<String>, problem: &str) -> OrchestratorError {
    OrchestratorError::config(file, field, problem)
}
