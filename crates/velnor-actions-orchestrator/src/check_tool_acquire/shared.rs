// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_contract::config::{QualifiedTool, QualifiedToolPlatform};
use velnor_actions_mise::{CheckDeadline, QualifiedCheck};

use super::{archive, layout, tree};
use super::observe::{
    check_deadline, read_with_deadline, run, sha256_with_deadline, staged_read_error,
};
use crate::OrchestratorError;
use crate::internal::internal;

static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

pub(super) fn publish_shared_prefix(
    shared_home: &Path,
    prefix: &Path,
    expected_tree: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    if !shared_home.is_absolute() || !prefix.is_absolute() {
        return Err(internal("qualified_tool_shared_paths"));
    }
    std::fs::create_dir_all(shared_home).map_err(|_| internal("qualified_tool_shared_home"))?;
    let staging = shared_home.join(format!(
        "staging-{}-{}",
        std::process::id(),
        NEXT_STAGING.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::symlink_metadata(&staging).is_ok() {
        return Err(internal("qualified_tool_shared_staging"));
    }
    let staged = layout::clone_prefix_tree(prefix, &staging, deadline)
        .and_then(|()| tree::tree_sha256(&staging, deadline));
    let staged = match staged {
        Ok(tree) if tree == expected_tree => staging,
        Ok(_) => {
            remove_shared_path(&staging)?;
            return Err(internal("qualified_tool_install_tree_sha256"));
        }
        Err(error) => {
            remove_shared_path(&staging)?;
            return Err(error);
        }
    };
    replace_shared_prefix(shared_home, &staged)
}

fn replace_shared_prefix(shared_home: &Path, staging: &Path) -> Result<(), OrchestratorError> {
    let prefix = shared_home.join("prefix");
    match std::fs::symlink_metadata(&prefix) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(&prefix)
                .map_err(|_| internal("qualified_tool_shared_replace"))?;
        }
        _ => return Err(internal("qualified_tool_shared_prefix")),
    }
    std::fs::rename(staging, &prefix).map_err(|_| internal("qualified_tool_shared_replace"))
}

fn remove_shared_path(path: &Path) -> Result<(), OrchestratorError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path).map_err(|_| internal("qualified_tool_shared_cleanup"))
        }
        _ => Err(internal("qualified_tool_shared_cleanup")),
    }
}

pub(super) fn fetch_archives(
    handle: &QualifiedCheck,
    tool: &QualifiedTool,
    qualified: &QualifiedToolPlatform,
    tool_home: &Path,
    deadline: CheckDeadline,
    expanded: &mut archive::ArchiveBudget,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), OrchestratorError> {
    let primary = fetch_extract(
        handle,
        tool,
        false,
        &qualified.artifacts,
        tool_home,
        deadline,
        expanded,
    )?;
    let dependencies = fetch_extract(
        handle,
        tool,
        true,
        &qualified.dependency_artifacts,
        tool_home,
        deadline,
        expanded,
    )?;
    Ok((primary, dependencies))
}

fn fetch_extract(
    handle: &QualifiedCheck,
    tool: &QualifiedTool,
    dependency: bool,
    artifacts: &[velnor_actions_contract::config::QualifiedToolArtifact],
    tool_home: &Path,
    deadline: CheckDeadline,
    expanded: &mut archive::ArchiveBudget,
) -> Result<Vec<PathBuf>, OrchestratorError> {
    let mut roots = Vec::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        let shared_archive = handle
            .qualified_shared_archive(&tool.id, &artifact.sha256)
            .map_err(|e| internal(&e.to_string()))?;
        let downloaded = tool_home.join("downloads").join(&artifact.sha256);
        let reused = reuse_shared_archive(&shared_archive, &artifact.sha256, deadline)?;
        let bytes = if let Some(bytes) = reused {
            bytes
        } else {
            run(
                handle.qualified_fetch_command(&tool.id, dependency, index),
                deadline,
            )?;
            let bytes = read_with_deadline(&downloaded, 1024 * 1024 * 1024, deadline)
                .map_err(|error| staged_read_error(error, "qualified_tool_artifact_unreadable"))?;
            if sha256_with_deadline(&bytes, deadline)? != artifact.sha256 {
                return Err(internal("qualified_tool_artifact_sha256"));
            }
            store_shared_archive(&shared_archive, &bytes, deadline)?;
            std::fs::remove_file(&downloaded)
                .map_err(|_| internal("qualified_tool_archive_cleanup"))?;
            bytes
        };
        let name = format!(
            "{}-{index}",
            if dependency { "dependency" } else { "primary" }
        );
        let verified = tool_home.join("verified").join(&name);
        crate::exclusive_write::write_exclusive_until(
            &verified,
            &bytes,
            "qualified_tool_archive",
            || check_deadline(deadline),
        )?;
        let root = tool_home.join("unpacked").join(name);
        archive::extract_archive(&verified, &root, &artifact.url, expanded, deadline)?;
        std::fs::remove_file(&verified).map_err(|_| internal("qualified_tool_archive_cleanup"))?;
        roots.push(root);
    }
    Ok(roots)
}

fn reuse_shared_archive(
    path: &Path,
    sha256: &str,
    deadline: CheckDeadline,
) -> Result<Option<Vec<u8>>, OrchestratorError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(internal("qualified_tool_shared_archive")),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(internal("qualified_tool_shared_archive"));
    }
    if metadata.len() > archive::MAX_ARCHIVE_BYTES {
        return Err(internal("qualified_tool_shared_archive"));
    }
    let bytes = read_with_deadline(path, archive::MAX_ARCHIVE_BYTES, deadline)
        .map_err(|error| staged_read_error(error, "qualified_tool_artifact_unreadable"))?;
    if sha256_with_deadline(&bytes, deadline)? != sha256 {
        return Err(internal("qualified_tool_shared_archive"));
    }
    Ok(Some(bytes))
}

fn store_shared_archive(
    path: &Path,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    if !path.is_absolute() {
        return Err(internal("qualified_tool_shared_paths"));
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > archive::MAX_ARCHIVE_BYTES {
        return Err(internal("qualified_tool_archive_input_limit"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| internal("qualified_tool_shared_archive"))?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            std::io::Write::write_all(&mut file, bytes)
                .map_err(|_| internal("qualified_tool_shared_archive"))?;
            check_deadline(deadline)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match reuse_shared_archive(path, &sha256_with_deadline(bytes, deadline)?, deadline)? {
                Some(_) => Ok(()),
                None => Err(internal("qualified_tool_shared_archive")),
            }
        }
        Err(_) => Err(internal("qualified_tool_shared_archive")),
    }
}
