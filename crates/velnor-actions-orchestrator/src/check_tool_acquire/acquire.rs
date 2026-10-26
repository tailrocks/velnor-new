// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_actions_contract::config::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolOptions, QualifiedToolPlatform,
};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, QualifiedCheck};

use super::super::cache_execution_context;
use super::{archive, layout, tree};
use super::observe::{
    check_deadline, compiler_prefix, executable_sha256, observe_executables, run,
    validate_installations,
};
use super::shared::{fetch_archives, publish_shared_prefix};
use crate::OrchestratorError;
use crate::check_evidence::gate::tools::{QualifiedToolReceipt, receipt};
use crate::internal::internal;

pub(crate) fn acquire(
    handle: &QualifiedCheck,
    check: &DiscoveredCheck,
    home: &Path,
    deadline: CheckDeadline,
) -> Result<Vec<QualifiedToolReceipt>, OrchestratorError> {
    let mut receipts = Vec::new();
    let mut expanded = archive::ArchiveBudget::new();
    let shared_root = shared_tool_root(home)?;
    for tool in &check.qualified_tools {
        receipts.push(hydrate_tool(
            handle,
            check,
            home,
            &shared_root,
            tool,
            &mut expanded,
            deadline,
        )?);
    }
    validate_installations(
        &check.qualified_tools,
        check.check.runner.platform,
        home,
        deadline,
    )?;
    Ok(receipts)
}

fn hydrate_tool(
    handle: &QualifiedCheck,
    check: &DiscoveredCheck,
    home: &Path,
    shared_root: &Path,
    tool: &QualifiedTool,
    expanded: &mut archive::ArchiveBudget,
    deadline: CheckDeadline,
) -> Result<QualifiedToolReceipt, OrchestratorError> {
    let qualified = tool
        .platforms
        .iter()
        .find(|p| p.platform == check.check.runner.platform)
        .ok_or_else(|| internal("qualified_tool_platform"))?;
    let tool_home = handle
        .qualified_tool_home(&tool.id)
        .map_err(|e| internal(&e.to_string()))?;
    let shared_home = handle
        .qualified_shared_tool_home(&tool.id)
        .map_err(|e| internal(&e.to_string()))?;
    if shared_home != shared_root.join(&tool.id) {
        return Err(internal("qualified_tool_shared_context"));
    }
    for directory in ["downloads", "verified", "unpacked", "cargo", "target"] {
        crate::exclusive_write::create_dir_no_symlink(home, &tool_home.join(directory))?;
    }
    let prefix = tool_home.join("prefix");
    let shared_prefix = shared_home.join("prefix");
    let warm = restore_shared_prefix(qualified, &shared_prefix, &prefix, deadline)?;
    if !warm {
        let (primary, dependencies) =
            fetch_archives(handle, tool, qualified, &tool_home, deadline, expanded)?;
        if source_mode(tool) {
            check_deadline(deadline)?;
            layout::prepare_cargo_source(
                tool,
                check.check.runner.platform,
                &tool_home,
                &primary,
                &dependencies,
                deadline,
            )?;
            check_deadline(deadline)?;
            run(handle.qualified_source_command(&tool.id), deadline)?;
        } else {
            check_deadline(deadline)?;
            layout::normalize_payload(
                tool,
                check.check.runner.platform,
                &primary,
                &prefix,
                deadline,
            )?;
            check_deadline(deadline)?;
        }
    }
    tree::freeze_tree(&prefix, deadline)?;
    let actual_tree = tree::tree_sha256(&prefix, deadline)?;
    if actual_tree != qualified.install_tree_sha256 {
        return Err(internal("qualified_tool_install_tree_sha256"));
    }
    let proofs = observe_executables(
        tool,
        check,
        home,
        &prefix,
        compiler_prefix(tool, &check.qualified_tools, home),
        deadline,
    )?;
    let proof = receipt(
        tool,
        check.check.runner.platform,
        qualified.artifacts.clone(),
        qualified.dependency_artifacts.clone(),
        actual_tree,
        proofs,
    )?;
    run(handle.qualified_link_command(&tool.id), deadline)?;
    if !warm {
        publish_shared_prefix(
            &shared_home,
            &prefix,
            &qualified.install_tree_sha256,
            deadline,
        )?;
    }
    Ok(proof)
}

fn shared_tool_root(home: &Path) -> Result<PathBuf, OrchestratorError> {
    if !cache_execution_context::is_private_check_home(home) {
        return Err(internal("qualified_tool_home_not_private"));
    }
    let temp = home
        .parent()
        .ok_or_else(|| internal("qualified_tool_home_without_temp"))?;
    let roots = cache_execution_context::resolve_roots(temp)?;
    for root in roots.all() {
        if root == home || home.starts_with(root) || root.starts_with(home) {
            return Err(internal("qualified_tool_shared_overlap"));
        }
    }
    Ok(roots.tool_root)
}

fn restore_shared_prefix(
    qualified: &QualifiedToolPlatform,
    shared_prefix: &Path,
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    if !shared_prefix_ready(qualified, shared_prefix, deadline)? {
        return Ok(false);
    }
    layout::clone_prefix_tree(shared_prefix, prefix, deadline)?;
    Ok(true)
}

fn shared_prefix_ready(
    qualified: &QualifiedToolPlatform,
    shared_prefix: &Path,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let metadata = match std::fs::symlink_metadata(shared_prefix) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(internal("qualified_tool_shared_prefix")),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("qualified_tool_shared_prefix"));
    }
    if tree::tree_sha256(shared_prefix, deadline)? != qualified.install_tree_sha256 {
        return Ok(false);
    }
    for executable in &qualified.executables {
        if executable_sha256(shared_prefix, &executable.path, deadline)? != executable.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn source_mode(tool: &QualifiedTool) -> bool {
    matches!(
        &tool.options,
        QualifiedToolOptions::Cargo {
            installation: QualifiedCargoInstallation::Source { .. },
            ..
        }
    )
}
