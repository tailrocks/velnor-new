// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::internal::internal;
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckPlatform, QualifiedTool, QualifiedToolBackend, QualifiedToolRecipe,
};
use velnor_actions_mise::CheckDeadline;

#[path = "check_tool_cargo_vendor.rs"]
mod cargo;
#[path = "check_tool_rust_layout.rs"]
mod rust;

pub(super) fn prepare_cargo_source(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    home: &Path,
    primary: &[PathBuf],
    dependencies: &[PathBuf],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    tool.validate("qualified_tools", &tool.id)
        .map_err(crate::internal::internal_contract)?;
    cargo::prepare_cargo_source(tool, platform, home, primary, dependencies, deadline)
}

pub(crate) fn normalize_payload(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    roots: &[PathBuf],
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    tool.validate("qualified_tools", &tool.id)
        .map_err(crate::internal::internal_contract)?;
    normalize_recipe_payload(
        &QualifiedToolRecipe::from(tool),
        platform,
        roots,
        prefix,
        deadline,
    )
}

pub(crate) fn normalize_recipe_payload(
    tool: &QualifiedToolRecipe,
    platform: CheckPlatform,
    roots: &[PathBuf],
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    tool.validate("qualification_recipe", &tool.id)
        .map_err(crate::internal::internal_contract)?;
    if !prefix.is_absolute() || roots.iter().any(|root| !root.is_absolute()) {
        return Err(internal("qualified_payload_requires_absolute_paths"));
    }
    if matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust") {
        return rust::normalize_rust_payload(tool, platform, roots, prefix, deadline);
    }
    let qualified = tool
        .platforms
        .iter()
        .find(|p| p.platform == platform)
        .ok_or_else(|| internal("qualified_payload_platform"))?;
    if roots.len() != 1 {
        return Err(internal(
            "qualified_payload_requires_single_release_archive",
        ));
    }
    let root = &roots[0];
    let mut candidates = Vec::new();
    if matches_executables(root, qualified, deadline)? {
        candidates.push(root.clone());
    }
    for entry in std::fs::read_dir(root).map_err(|_| internal("qualified_payload_root"))? {
        check_deadline(deadline)?;
        let entry = entry.map_err(|_| internal("qualified_payload_root"))?;
        if entry
            .file_type()
            .map_err(|_| internal("qualified_payload_root"))?
            .is_dir()
            && matches_executables(&entry.path(), qualified, deadline)?
        {
            candidates.push(entry.path());
        }
    }
    if candidates.len() != 1 {
        return Err(internal("qualified_payload_ambiguous_layout"));
    }
    if std::fs::symlink_metadata(prefix).is_ok() {
        return Err(internal("qualified_payload_destination_exists"));
    }
    std::fs::rename(&candidates[0], prefix).map_err(|_| internal("qualified_payload_move"))
}

pub(crate) fn clone_prefix_tree(
    source: &Path,
    destination: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    if !source.is_absolute() || !destination.is_absolute() {
        return Err(internal("qualified_prefix_requires_absolute_paths"));
    }
    let metadata = std::fs::symlink_metadata(source).map_err(|_| internal("qualified_prefix"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("qualified_prefix"));
    }
    if std::fs::symlink_metadata(destination).is_ok() {
        return Err(internal("qualified_prefix_destination_exists"));
    }
    std::fs::create_dir(destination).map_err(|_| internal("qualified_prefix_clone"))?;
    clone_mode(source, destination)?;
    clone_children(source, destination, deadline)
}

fn clone_children(
    source: &Path,
    destination: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for entry in std::fs::read_dir(source).map_err(|_| internal("qualified_prefix_clone"))? {
        check_deadline(deadline)?;
        let entry = entry.map_err(|_| internal("qualified_prefix_clone"))?;
        let target = destination.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|_| internal("qualified_prefix_clone"))?;
        if kind.is_symlink() {
            clone_link(&entry.path(), &target)?;
        } else if kind.is_dir() {
            std::fs::create_dir(&target).map_err(|_| internal("qualified_prefix_clone"))?;
            clone_mode(&entry.path(), &target)?;
            clone_children(&entry.path(), &target, deadline)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).map_err(|_| internal("qualified_prefix_clone"))?;
            clone_mode(&entry.path(), &target)?;
        } else {
            return Err(internal("qualified_prefix_special"));
        }
        check_deadline(deadline)?;
    }
    Ok(())
}

#[cfg(unix)]
fn clone_link(source: &Path, target: &Path) -> Result<(), OrchestratorError> {
    let link = std::fs::read_link(source).map_err(|_| internal("qualified_prefix_clone"))?;
    std::os::unix::fs::symlink(&link, target).map_err(|_| internal("qualified_prefix_clone"))
}

#[cfg(not(unix))]
fn clone_link(source: &Path, target: &Path) -> Result<(), OrchestratorError> {
    let _ = (source, target);
    Err(internal("qualified_prefix_link_platform"))
}

#[cfg(unix)]
fn clone_mode(source: &Path, target: &Path) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(source)
        .map_err(|_| internal("qualified_prefix_clone"))?
        .permissions()
        .mode();
    std::fs::set_permissions(target, std::fs::Permissions::from_mode(mode))
        .map_err(|_| internal("qualified_prefix_clone"))
}

#[cfg(not(unix))]
fn clone_mode(source: &Path, target: &Path) -> Result<(), OrchestratorError> {
    let _ = (source, target);
    Ok(())
}

fn matches_executables(
    root: &Path,
    qualified: &velnor_actions_contract::config::QualifiedToolRecipePlatform,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let metadata =
        std::fs::symlink_metadata(root).map_err(|_| internal("qualified_payload_root"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(internal("qualified_payload_root_symlink"));
    }
    for executable in &qualified.executables {
        check_deadline(deadline)?;
        crate::check_evidence::reject_link_components(root, &executable.path)?;
        match std::fs::symlink_metadata(root.join(&executable.path)) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(internal("qualified_payload_executable")),
        }
    }
    Ok(!qualified.executables.is_empty())
}

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}
