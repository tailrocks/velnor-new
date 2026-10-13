//! Bounded identity and immutable projection of an installed tool tree.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};
use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;

const MAX_TREE_ENTRIES: usize = 100_000;
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TREE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const READ_CHUNK: usize = 64 * 1024;

#[derive(Debug)]
enum Entry {
    File(FileEntry),
    Symlink(String, String),
    Directory(String),
}

#[derive(Debug)]
struct FileEntry {
    path: String,
    sha256: String,
    executable: bool,
    mode: u32,
}

impl Entry {
    fn path(&self) -> &str {
        match self {
            Self::File(file) => &file.path,
            Self::Symlink(path, _) | Self::Directory(path) => path,
        }
    }
}

struct Walk {
    entries: Vec<Entry>,
    bytes: u64,
}

impl Walk {
    fn add(&mut self, entry: Entry) -> Result<(), OrchestratorError> {
        if self.entries.len() >= MAX_TREE_ENTRIES {
            return Err(crate::internal::internal("tool_tree_entry_limit"));
        }
        self.entries.push(entry);
        Ok(())
    }
}
/// Compute canonical SHA-256 over every relative entry in an installation tree.
pub(super) fn tree_sha256(
    root: &Path,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let root = validate_root(root)?;
    tree_digest(&collect(&root, deadline)?, deadline)
}
/// Validate a tree and make every regular file owner-readonly, preserving execute state.
pub(super) fn freeze_tree(root: &Path, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let root = validate_root(root)?;
    for entry in collect(&root, deadline)? {
        super::check_deadline(deadline)?;
        match entry {
            Entry::File(file) => readonly_file(&root.join(&file.path), file.mode)?,
            Entry::Directory(_) | Entry::Symlink(_, _) => {}
        }
    }
    Ok(())
}
fn validate_root(root: &Path) -> Result<PathBuf, OrchestratorError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| io_error(root, error))?;
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path(root, "root_symlink"));
    }
    if !metadata.is_dir() {
        return Err(unsafe_path(root, "root_not_directory"));
    }
    root.canonicalize().map_err(|error| io_error(root, error))
}
fn collect(root: &Path, deadline: CheckDeadline) -> Result<Vec<Entry>, OrchestratorError> {
    let mut walk = Walk {
        entries: Vec::new(),
        bytes: 0,
    };
    walk_directory(root, root, &mut walk, deadline)?;
    super::check_deadline(deadline)?;
    walk.entries
        .sort_by(|left, right| left.path().cmp(right.path()));
    super::check_deadline(deadline)?;
    Ok(walk.entries)
}
fn walk_directory(
    root: &Path,
    directory: &Path,
    walk: &mut Walk,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for item in fs::read_dir(directory).map_err(|error| io_error(directory, error))? {
        super::check_deadline(deadline)?;
        let path = item.map_err(|error| io_error(directory, error))?.path();
        let relative = relative_path(root, &path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if metadata.file_type().is_symlink() {
            walk.add(Entry::Symlink(relative, link_target(root, &path)?))?;
        } else if metadata.is_dir() {
            let resolved = path
                .canonicalize()
                .map_err(|error| io_error(&path, error))?;
            if !resolved.starts_with(root) {
                return Err(unsafe_path(&path, "root_escape"));
            }
            walk.add(Entry::Directory(relative))?;
            walk_directory(root, &path, walk, deadline)?;
        } else if metadata.is_file() {
            let file = read_file(&path, MAX_TREE_BYTES - walk.bytes, deadline)?;
            walk.bytes = walk.bytes.saturating_add(file.length);
            walk.add(Entry::File(FileEntry {
                path: relative,
                sha256: file.sha256,
                executable: file.mode & 0o111 != 0,
                mode: file.mode,
            }))?;
        } else {
            return Err(unsafe_path(&path, "special_file"));
        }
    }
    Ok(())
}
fn read_file(
    path: &Path,
    tree_remaining: u64,
    deadline: CheckDeadline,
) -> Result<ReadFile, OrchestratorError> {
    let limit = MAX_FILE_BYTES.min(tree_remaining);
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::LOOP {
            unsafe_path(path, "symlink_refused")
        } else {
            io_error(path, std::io::Error::from(error))
        }
    })?;
    let stat =
        rustix::fs::fstat(&fd).map_err(|error| io_error(path, std::io::Error::from(error)))?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
        return Err(unsafe_path(path, "special_file"));
    }
    let mode = stat_mode(stat.st_mode);
    let mut file = fs::File::from(fd);
    let mut hash = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = vec![0_u8; READ_CHUNK];
    loop {
        super::check_deadline(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        super::check_deadline(deadline)?;
        if count == 0 {
            break;
        }
        length = length.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        if length > limit {
            return Err(crate::internal::internal("tool_tree_size_limit"));
        }
        hash.update(&buffer[..count]);
    }
    Ok(ReadFile {
        sha256: digest_hex(&hash.finalize()),
        mode,
        length,
    })
}

struct ReadFile {
    sha256: String,
    mode: u32,
    length: u64,
}

#[cfg(target_os = "linux")]
fn stat_mode(mode: u32) -> u32 {
    mode
}

#[cfg(target_os = "macos")]
fn stat_mode(mode: u16) -> u32 {
    u32::from(mode)
}

fn link_target(root: &Path, path: &Path) -> Result<String, OrchestratorError> {
    let target = fs::read_link(path).map_err(|error| io_error(path, error))?;
    let text = target
        .to_str()
        .ok_or_else(|| unsafe_path(path, "non_utf8_symlink"))?;
    if text.is_empty() || target.is_absolute() || text.contains('\\') {
        return Err(unsafe_path(path, "non_relative_symlink"));
    }
    let resolved = path
        .parent()
        .ok_or_else(|| unsafe_path(path, "symlink_parent"))?
        .join(&target)
        .canonicalize()
        .map_err(|error| io_error(path, error))?;
    if !resolved.starts_with(root) {
        return Err(unsafe_path(path, "symlink_root_escape"));
    }
    let metadata = fs::symlink_metadata(&resolved).map_err(|error| io_error(&resolved, error))?;
    if !metadata.is_file() {
        return Err(unsafe_path(path, "symlink_special_target"));
    }
    Ok(text.to_owned())
}

fn relative_path(root: &Path, path: &Path) -> Result<String, OrchestratorError> {
    let mut parts = Vec::new();
    for component in path
        .strip_prefix(root)
        .map_err(|_| unsafe_path(path, "root_escape"))?
        .components()
    {
        let Component::Normal(component) = component else {
            return Err(unsafe_path(path, "invalid_relative_path"));
        };
        let component = component
            .to_str()
            .ok_or_else(|| unsafe_path(path, "non_utf8_path"))?;
        if component.contains('\\') {
            return Err(unsafe_path(path, "non_posix_path"));
        }
        parts.push(component);
    }
    if parts.is_empty() {
        Err(unsafe_path(path, "root_entry"))
    } else {
        Ok(parts.join("/"))
    }
}

fn tree_digest(entries: &[Entry], deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    let mut hash = Sha256::new();
    hash.update(b"[");
    for (index, entry) in entries.iter().enumerate() {
        super::check_deadline(deadline)?;
        let value = match entry {
            Entry::File(file) => {
                json!({"path": &file.path, "kind": "file", "sha256": &file.sha256, "executable": file.executable})
            }
            Entry::Symlink(path, target) => {
                json!({"path": path, "kind": "symlink", "target": target})
            }
            Entry::Directory(path) => json!({"path": path, "kind": "directory"}),
        };
        if index != 0 {
            hash.update(b",");
        }
        let bytes = canonical_json_bytes(&value)
            .map_err(|_| crate::internal::internal("tool_tree_manifest"))?;
        hash.update(&bytes);
    }
    super::check_deadline(deadline)?;
    hash.update(b"]");
    Ok(digest_hex(&hash.finalize()))
}

fn readonly_file(path: &Path, mode: u32) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    let permissions = std::os::unix::fs::PermissionsExt::from_mode(
        0o400 | (u32::from(mode & 0o111 != 0) * 0o100),
    );
    #[cfg(not(unix))]
    let permissions = {
        let mut permissions = fs::metadata(path)
            .map_err(|error| io_error(path, error))?
            .permissions();
        permissions.set_readonly(true);
        permissions
    };
    fs::set_permissions(path, permissions).map_err(|error| io_error(path, error))
}

fn digest_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
            out
        })
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
#[cfg(test)]
mod tests;
