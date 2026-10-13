use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;
use velnor_actions_orchestrator_core::sha256::sha256_hex;

const MAX_ENTRIES: usize = 1_000;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const READ_CHUNK: usize = 64 * 1024;

mod write;
#[cfg(test)]
pub(super) use write::populate;
pub(super) use write::{create_destination, populate_until, set_directory_mode, verify_owned_root};

pub(super) struct Entry {
    pub(super) path: String,
    pub(super) mode: u32,
    pub(super) kind: Kind,
}

pub(super) enum Kind {
    Directory,
    File {
        sha256: String,
        executable: bool,
        bytes: Vec<u8>,
    },
}

struct Walk {
    entries: Vec<Entry>,
    bytes: u64,
}

#[cfg(test)]
pub(super) fn collect_tree(root: &Path) -> Result<Vec<Entry>, OrchestratorError> {
    collect_tree_until(root, None)
}

pub(super) fn collect_tree_until(
    root: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<Vec<Entry>, OrchestratorError> {
    checkpoint(deadline)?;
    let root = super::existing_directory(root, "sdk_tree_root")?;
    let root_mode =
        metadata_mode(&fs::symlink_metadata(&root).map_err(|error| io_error(&root, error))?);
    reject_special_mode(&root, root_mode)?;
    let mut walk = Walk {
        entries: Vec::new(),
        bytes: 0,
    };
    visit(&root, &root, &mut walk, deadline)?;
    checkpoint(deadline)?;
    walk.entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(walk.entries)
}

fn visit(
    root: &Path,
    directory: &Path,
    walk: &mut Walk,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    for item in fs::read_dir(directory).map_err(|error| io_error(directory, error))? {
        checkpoint(deadline)?;
        let path = item.map_err(|error| io_error(directory, error))?.path();
        let relative = relative_path(root, &path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(unsafe_path(&path, "sdk_symlink_rejected"));
        }
        if metadata.is_dir() {
            let directory_mode = metadata_mode(&metadata);
            reject_special_mode(&path, directory_mode)?;
            add(
                walk,
                Entry {
                    path: relative,
                    mode: directory_mode & 0o777,
                    kind: Kind::Directory,
                },
            )?;
            visit(root, &path, walk, deadline)?;
        } else if metadata.is_file() {
            let file = read_regular(&path, MAX_BYTES.saturating_sub(walk.bytes), true, deadline)?;
            walk.bytes = walk.bytes.saturating_add(file.length);
            add(
                walk,
                Entry {
                    path: relative,
                    mode: file.mode,
                    kind: Kind::File {
                        sha256: file.sha256,
                        executable: file.mode & 0o111 != 0,
                        bytes: file.bytes,
                    },
                },
            )?;
        } else {
            return Err(unsafe_path(&path, "sdk_special_file"));
        }
        checkpoint(deadline)?;
    }
    Ok(())
}

fn add(walk: &mut Walk, entry: Entry) -> Result<(), OrchestratorError> {
    if walk.entries.len() >= MAX_ENTRIES {
        return Err(internal("sdk_tree_entry_limit"));
    }
    walk.entries.push(entry);
    Ok(())
}

struct FileData {
    sha256: String,
    mode: u32,
    length: u64,
    bytes: Vec<u8>,
}

pub(super) fn file_hash_until(
    path: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<String, OrchestratorError> {
    Ok(read_regular(path, MAX_BYTES, false, deadline)?.sha256)
}

fn read_regular(
    path: &Path,
    remaining: u64,
    retain: bool,
    deadline: Option<CheckDeadline>,
) -> Result<FileData, OrchestratorError> {
    checkpoint(deadline)?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::LOOP {
            unsafe_path(path, "sdk_symlink_rejected")
        } else {
            io_error(path, std::io::Error::from(error))
        }
    })?;
    let stat =
        rustix::fs::fstat(&fd).map_err(|error| io_error(path, std::io::Error::from(error)))?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
        return Err(unsafe_path(path, "sdk_special_file"));
    }
    let full_mode = stat_mode(stat.st_mode);
    reject_special_mode(path, full_mode)?;
    let mut file = fs::File::from(fd);
    let mut hash = Sha256::new();
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; READ_CHUNK];
    let mut length = 0_u64;
    loop {
        checkpoint(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(u64::try_from(count).unwrap_or(u64::MAX))
            .ok_or_else(|| internal("sdk_tree_size_limit"))?;
        if length > MAX_BYTES || length > remaining {
            return Err(internal("sdk_tree_size_limit"));
        }
        hash.update(&buffer[..count]);
        if retain {
            bytes.extend_from_slice(&buffer[..count]);
        }
        checkpoint(deadline)?;
    }
    Ok(FileData {
        sha256: hex(&hash.finalize()),
        mode: full_mode & 0o777,
        length,
        bytes,
    })
}

#[cfg(test)]
pub(super) fn tree_digest(entries: &[Entry]) -> Result<String, OrchestratorError> {
    tree_digest_until(entries, None)
}

pub(super) fn tree_digest_until(
    entries: &[Entry],
    deadline: Option<CheckDeadline>,
) -> Result<String, OrchestratorError> {
    checkpoint(deadline)?;
    let manifest = entries
        .iter()
        .map(|entry| match &entry.kind {
            Kind::Directory => {
                json!({"path": &entry.path, "kind": "directory", "mode": entry.mode & 0o777})
            }
            Kind::File {
                sha256, executable, ..
            } => json!({
                "path": &entry.path,
                "kind": "file",
                "mode": entry.mode & 0o777,
                "sha256": sha256,
                "executable": executable
            }),
        })
        .collect::<Vec<_>>();
    let bytes = canonical_json_bytes(&manifest).map_err(|_| internal("sdk_tree_manifest"))?;
    checkpoint(deadline)?;
    Ok(sha256_hex(&bytes))
}

pub(super) fn checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| velnor_actions_orchestrator_core::internal(&error.to_string()))?;
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> Result<String, OrchestratorError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| unsafe_path(path, "sdk_tree_escape"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(unsafe_path(path, "sdk_tree_path"));
        };
        let text = part
            .to_str()
            .ok_or_else(|| unsafe_path(path, "sdk_non_utf8_path"))?;
        if text.is_empty() || text.contains('\\') || text.contains('\0') {
            return Err(unsafe_path(path, "sdk_tree_path"));
        }
        parts.push(text);
    }
    if parts.is_empty() {
        return Err(unsafe_path(path, "sdk_tree_root"));
    }
    Ok(parts.join("/"))
}

fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        metadata.mode()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

pub(super) fn reject_special_mode(path: &Path, mode: u32) -> Result<(), OrchestratorError> {
    if mode & 0o7000 != 0 {
        return Err(unsafe_path(path, "sdk_special_mode"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn stat_mode(value: u32) -> u32 {
    value
}

#[cfg(target_os = "macos")]
fn stat_mode(value: u16) -> u32 {
    u32::from(value)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

pub(super) fn io_error(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

pub(super) fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
