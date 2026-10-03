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
use std::io::{Read, Write};
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
pub(super) fn read_checked(path: &Path, limit: u64, label: &str) -> io::Result<Vec<u8>> {
    let before_meta = fs::symlink_metadata(path).map_err(|error| with_path(error, label, path))?;
    let before = regular_stamp(&before_meta, label)?;
    if before.size > limit {
        return Err(invalid(label, "size_limit"));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW_FLAG)
        .open(path)
        .map_err(|error| with_path(error, label, path))?;
    let fd_before = regular_stamp(
        &file
            .metadata()
            .map_err(|error| with_path(error, label, path))?,
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
        .map_err(|error| with_path(error, label, path))?;
    let byte_count = u64::try_from(bytes.len()).map_err(|_| invalid(label, "size_overflow"))?;
    if byte_count > limit || byte_count != before.size {
        return Err(invalid(label, "changed_during_read"));
    }

    let fd_after = regular_stamp(
        &file
            .metadata()
            .map_err(|error| with_path(error, label, path))?,
        label,
    )?;
    let path_after = regular_stamp(
        &fs::symlink_metadata(path).map_err(|error| with_path(error, label, path))?,
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
        Err(error) => Err(with_path(error, label, path)),
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
        Err(error) => Err(with_path(error, label, path)),
    }
}

/// Create an exclusive private temporary directory with owner-only access.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn create_private_directory() -> io::Result<PathBuf> {
    let base = absolute_temp_directory()?;
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
            Ok(()) => match verify_private_directory(&directory) {
                Ok(()) => return Ok(directory),
                Err(error) => {
                    remove_created_directory(&directory);
                    return Err(error);
                }
            },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(with_path(error, "directory_create", &directory)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "private_git_index:directory_collision_limit",
    ))
}

/// Copy validated bytes into a new owner-only index file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn write_private_index(directory: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let path = directory.join("index");
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .custom_flags(NOFOLLOW_FLAG)
        .mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| with_path(error, "private_index_create", &path))?;
    file.write_all(bytes)
        .map_err(|error| with_path(error, "private_index_write", &path))?;
    drop(file);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| with_path(error, "private_index_verify", &path))?;
    let stamp = regular_stamp(&metadata, "private_index_verify")?;
    let mode = metadata.permissions().mode() & 0o777;
    let expected = u64::try_from(bytes.len()).map_err(|_| invalid("private_index", "size"))?;
    if stamp.size != expected || mode & 0o077 != 0 || mode & 0o600 != 0o600 {
        return Err(invalid("private_index", "metadata_changed"));
    }
    Ok(path)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn absolute_temp_directory() -> io::Result<PathBuf> {
    let raw = std::env::temp_dir();
    let absolute = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir()
            .map_err(|error| with_path(error, "temp_current_dir", &raw))?
            .join(raw)
    };
    absolute
        .canonicalize()
        .map_err(|error| with_path(error, "temp_directory", &absolute))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_private_directory(path: &Path) -> io::Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| with_path(error, "directory_verify", path))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(invalid("directory_verify", "not_directory"));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid("directory_verify", "not_private"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn remove_created_directory(path: &Path) {
    match fs::remove_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("private_git_index:directory_cleanup:{}", error),
    }
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

fn with_path(error: io::Error, label: &str, path: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("private_git_index:{label}:{}:{error}", path.display()),
    )
}
