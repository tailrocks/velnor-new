use crate::OrchestratorError;
use crate::internal::internal;
use std::fs::{self, Metadata};
use std::path::{Component, Path};
use velnor_actions_contract::config::{
    MAX_CHECK_CONTAINER_RUNTIME_ENTRIES, MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES,
};
use velnor_actions_mise::CheckDeadline;

use super::{RuntimeEntryEvidence, RuntimeEntryKind};

const MAX_RUNTIME_METADATA_BYTES: u64 = 64 * 1024;

/// Stable identity of the declared Unix socket endpoint.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SocketEvidence {
    /// Canonical absolute socket path.
    pub path: String,
    /// Owning Unix user ID.
    pub owner: u32,
    /// Owning Unix group ID.
    pub group: u32,
    /// Socket mode, including permission bits.
    pub mode: u32,
    /// Device identity containing the socket inode.
    pub device: u64,
    /// Socket inode identity.
    pub inode: u64,
}

/// Stable identity of the declared `OrbStack` runtime directory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeRootEvidence {
    /// Canonical absolute runtime directory path.
    pub path: String,
    /// Owning Unix user ID.
    pub owner: u32,
    /// Owning Unix group ID.
    pub group: u32,
    /// Full directory mode.
    pub mode: u32,
    /// Device identity containing the directory inode.
    pub device: u64,
    /// Runtime directory inode identity.
    pub inode: u64,
}

/// Fresh identity evidence captured after runtime revalidation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeObservation {
    /// Fresh socket identity.
    pub socket: SocketEvidence,
    /// Fresh `OrbStack` runtime directory identity.
    pub runtime_root: Option<RuntimeRootEvidence>,
}

pub(super) fn validate_socket(
    path: &Path,
    expected_owner: u32,
) -> Result<SocketEvidence, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    if metadata.file_type().is_symlink() || !is_socket(&metadata) {
        return Err(internal("container_socket_not_unix_socket"));
    }
    if owner(&metadata) != expected_owner {
        return Err(internal("container_socket_owner"));
    }
    let canonical = path.canonicalize().map_err(|e| io(path, e))?;
    if canonical != path {
        return Err(internal("container_socket_not_canonical"));
    }
    Ok(SocketEvidence {
        path: canonical.display().to_string(),
        owner: expected_owner,
        group: group(&metadata),
        mode: mode(&metadata),
        device: device(&metadata),
        inode: inode(&metadata),
    })
}

pub(super) fn inspect_runtime_until(
    directory: &Path,
    expected_owner: u32,
    deadline: Option<CheckDeadline>,
) -> Result<(Vec<RuntimeEntryEvidence>, RuntimeRootEvidence), OrchestratorError> {
    checkpoint(deadline)?;
    let metadata = fs::symlink_metadata(directory).map_err(|e| io(directory, e))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || owner(&metadata) != expected_owner
    {
        return Err(internal("orbstack_runtime_root_owner"));
    }
    let root_mode = mode(&metadata);
    if root_mode & 0o022 != 0 {
        return Err(internal("orbstack_runtime_root_permissions"));
    }
    let root = RuntimeRootEvidence {
        path: directory.display().to_string(),
        owner: expected_owner,
        group: group(&metadata),
        device: device(&metadata),
        inode: inode(&metadata),
        mode: root_mode,
    };
    let mut entries = Vec::new();
    walk_runtime(directory, directory, expected_owner, &mut entries, deadline)?;
    if !entries
        .iter()
        .any(|entry| entry.path == "status" && entry.kind == RuntimeEntryKind::Directory)
        || !entries.iter().any(|entry| {
            entry.path == "vmgr.version" && entry.kind == RuntimeEntryKind::RegularFile
        })
    {
        return Err(internal("orbstack_runtime_required_metadata"));
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    checkpoint(deadline)?;
    Ok((entries, root))
}

fn walk_runtime(
    root: &Path,
    directory: &Path,
    expected_owner: u32,
    entries: &mut Vec<RuntimeEntryEvidence>,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    for entry in fs::read_dir(directory).map_err(|e| io(directory, e))? {
        checkpoint(deadline)?;
        if entries.len() >= MAX_CHECK_CONTAINER_RUNTIME_ENTRIES {
            return Err(internal("orbstack_runtime_entry_limit"));
        }
        let path = entry.map_err(|e| io(directory, e))?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        if metadata.file_type().is_symlink() {
            return Err(internal("orbstack_runtime_symlink"));
        }
        if owner(&metadata) != expected_owner {
            return Err(internal("orbstack_runtime_entry_owner"));
        }
        let kind = entry_kind(&metadata)?;
        if kind == RuntimeEntryKind::RegularFile && metadata.len() > MAX_RUNTIME_METADATA_BYTES {
            return Err(internal("orbstack_runtime_metadata_limit"));
        }
        entries.push(RuntimeEntryEvidence {
            path: relative_path(root, &path)?,
            kind,
            owner: expected_owner,
        });
        if kind == RuntimeEntryKind::Directory {
            walk_runtime(root, &path, expected_owner, entries, deadline)?;
        }
        checkpoint(deadline)?;
    }
    Ok(())
}

fn checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| crate::internal::internal(&error.to_string()))?;
    }
    Ok(())
}

fn entry_kind(metadata: &Metadata) -> Result<RuntimeEntryKind, OrchestratorError> {
    if metadata.is_dir() {
        return Ok(RuntimeEntryKind::Directory);
    }
    if metadata.is_file() {
        return Ok(RuntimeEntryKind::RegularFile);
    }
    #[cfg(unix)]
    if std::os::unix::fs::FileTypeExt::is_socket(&metadata.file_type()) {
        return Ok(RuntimeEntryKind::UnixSocket);
    }
    Err(internal("orbstack_runtime_special_file"))
}

fn relative_path(root: &Path, path: &Path) -> Result<String, OrchestratorError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| internal("orbstack_runtime_escape"))?;
    let mut output = String::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(internal("orbstack_runtime_relative_path"));
        };
        let part = part
            .to_str()
            .ok_or_else(|| internal("orbstack_runtime_utf8"))?;
        if part.contains('/') || part.contains('\\') || part.chars().any(char::is_control) {
            return Err(internal("orbstack_runtime_relative_path"));
        }
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(part);
        if output.len() > MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES {
            return Err(internal("orbstack_runtime_entry_path_limit"));
        }
    }
    if output.is_empty() {
        Err(internal("orbstack_runtime_root_entry"))
    } else {
        Ok(output)
    }
}

pub(super) fn owner(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::uid(metadata)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

fn group(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::gid(metadata)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

fn device(metadata: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::dev(metadata)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

fn inode(metadata: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::ino(metadata)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

fn mode(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::mode(metadata)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}

fn is_socket(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileTypeExt::is_socket(&metadata.file_type())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn io(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}
