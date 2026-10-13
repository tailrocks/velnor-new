//! Normalize retained verified payloads without multiplying extracted bytes.
use crate::OrchestratorError;
use crate::internal::internal;
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{CheckPlatform, QualifiedTool, QualifiedToolBackend};
use velnor_actions_mise::CheckDeadline;

mod cargo;
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

pub(super) fn normalize_payload(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    roots: &[PathBuf],
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    tool.validate("qualified_tools", &tool.id)
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

fn matches_executables(
    root: &Path,
    qualified: &velnor_actions_contract::config::QualifiedToolPlatform,
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

#[cfg(test)]
mod tests;
