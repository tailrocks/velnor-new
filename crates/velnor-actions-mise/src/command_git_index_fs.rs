//! Filesystem primitives for the private Git index guard.
//!
//! The supported targets use `O_NOFOLLOW` without a libc dependency. Reads
//! pin the source file identity before and after the bounded read; this closes
//! deterministic symlink and replacement plants while retaining the honest
//! same-user race boundary of path based Rust filesystem APIs.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::{self, DirBuilder, Metadata, OpenOptions};
use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Read;
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) const MAX_POINTER_BYTES: u64 = 4096;

#[cfg(target_os = "linux")]
const NOFOLLOW_FLAG: i32 = 0x20000;
#[cfg(target_os = "macos")]
const NOFOLLOW_FLAG: i32 = 0x100;
#[cfg(target_os = "linux")]
const NONBLOCK_FLAG: i32 = 0x800;
#[cfg(target_os = "macos")]
const NONBLOCK_FLAG: i32 = 0x4;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const DIRECTORY_ATTEMPTS: u32 = 32;
#[cfg(any(target_os = "linux", target_os = "macos"))]
static DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Stable metadata used to detect replacement or in-place mutation.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileStamp {
    dev: u64,
    ino: u64,
    nlink: u64,
    size: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl FileStamp {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            nlink: metadata.nlink(),
            size: metadata.size(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
        }
    }
}

/// Read a regular, singly linked file without following its final component.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) fn read_checked(
    path: &Path,
    limit: u64,
    label: &str,
) -> io::Result<Vec<u8>> {
    let before_meta = fs::symlink_metadata(path).map_err(|error| with_path(&error, label, path))?;
    let before = regular_stamp(&before_meta, label)?;
    if before.size > limit {
        return Err(invalid(label, "size_limit"));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW_FLAG | NONBLOCK_FLAG)
        .open(path)
        .map_err(|error| with_path(&error, label, path))?;
    let fd_before = regular_stamp(
        &file
            .metadata()
            .map_err(|error| with_path(&error, label, path))?,
        label,
    )?;
    same_stamp(&before, &fd_before, label)?;

    let read_limit = limit
        .checked_add(1)
        .ok_or_else(|| invalid(label, "size_limit_overflow"))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| with_path(&error, label, path))?;
    let byte_count = u64::try_from(bytes.len()).map_err(|_| invalid(label, "size_overflow"))?;
    if byte_count > limit || byte_count != before.size {
        return Err(invalid(label, "changed_during_read"));
    }

    let fd_after = regular_stamp(
        &file
            .metadata()
            .map_err(|error| with_path(&error, label, path))?,
        label,
    )?;
    let path_after = regular_stamp(
        &fs::symlink_metadata(path).map_err(|error| with_path(&error, label, path))?,
        label,
    )?;
    same_stamp(&before, &fd_after, label)?;
    same_stamp(&before, &path_after, label)?;
    Ok(bytes)
}

/// Validate a normal file without opening it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn normal_file(path: &Path, label: &str) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            regular_stamp(&metadata, label)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(with_path(&error, label, path)),
    }
}

/// Validate a non-symlink directory without following a metadata alias.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn normal_directory(path: &Path, label: &str) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(invalid(label, "symlink")),
        Ok(_) => Err(invalid(label, "not_directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(with_path(&error, label, path)),
    }
}

/// Create an exclusive private temporary directory with owner-only access.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) fn create_private_directory(excluded: &[&Path]) -> io::Result<PathBuf> {
    let base = absolute_temp_directory(excluded)?;
    let base_identity = directory_identity(&base)?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::other(format!("private_git_index:clock:{error}")))?;
    for _attempt in 0..DIRECTORY_ATTEMPTS {
        let counter = DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "velnor-git-index-{}-{}-{}-{}",
            std::process::id(),
            time.as_secs(),
            time.subsec_nanos(),
            counter
        );
        let directory = base.join(name);
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&directory) {
            Ok(()) => match verify_private_directory(&directory).and_then(|()| {
                if directory_identity(&base)? == base_identity {
                    Ok(())
                } else {
                    Err(invalid("temp_directory", "changed_identity"))
                }
            }) {
                Ok(()) => return Ok(directory),
                Err(error) => {
                    eprintln!("private_git_index:unbound_allocation_retained");
                    return Err(error);
                }
            },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(with_path(&error, "directory_create", &directory)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "private_git_index:directory_collision_limit",
    ))
}

/// Copy validated bytes into a new owner-only index file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn write_private_index(
    root: &super::super::private_root::PrivateGitRoot,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    let slot = super::super::private_root::OwnedFile::Index;
    root.write_file(slot, bytes)?;
    Ok(root.slot_path(slot).to_path_buf())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn absolute_temp_directory(excluded: &[&Path]) -> io::Result<PathBuf> {
    let base = Path::new("/tmp")
        .canonicalize()
        .map_err(|error| with_path(&error, "temp_directory", Path::new("/tmp")))?;
    if excluded.iter().any(|source| base.starts_with(source)) {
        return Err(invalid("temp_directory", "inside_source"));
    }
    for ancestor in base.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        let mode = metadata.permissions().mode();
        if !metadata.is_dir() || metadata.uid() != 0 {
            return Err(invalid("temp_directory", "untrusted_ancestor"));
        }
        let allowed_write = ancestor == base && mode & 0o1000 != 0;
        if mode & 0o022 != 0 && !allowed_write {
            return Err(invalid("temp_directory", "writable_ancestor"));
        }
    }
    Ok(base)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn directory_identity(path: &Path) -> io::Result<(u64, u64, u32, u32)> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(invalid("temp_directory", "invalid_type"));
    }
    Ok((
        metadata.dev(),
        metadata.ino(),
        metadata.uid(),
        metadata.mode(),
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_private_directory(path: &Path) -> io::Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| with_path(&error, "directory_verify", path))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(invalid("directory_verify", "not_directory"));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid("directory_verify", "not_private"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn regular_stamp(metadata: &Metadata, label: &str) -> io::Result<FileStamp> {
    if metadata.file_type().is_symlink() {
        return Err(invalid(label, "symlink"));
    }
    if !metadata.file_type().is_file() {
        return Err(invalid(label, "not_regular_file"));
    }
    let stamp = FileStamp::from_metadata(metadata);
    if stamp.nlink != 1 {
        return Err(invalid(label, "inode_alias"));
    }
    Ok(stamp)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn same_stamp(expected: &FileStamp, actual: &FileStamp, label: &str) -> io::Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(invalid(label, "changed_during_read"))
    }
}

fn invalid(label: &str, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("private_git_index:{label}:{detail}"),
    )
}

fn with_path(error: &io::Error, label: &str, path: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("private_git_index:{label}:{}:{error}", path.display()),
    )
}
