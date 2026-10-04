//! Closed, race-aware source inventory for the forbidden-source scanner.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::OsStr;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::{self, File, Metadata, OpenOptions};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Read;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::{Component, Path, PathBuf};

#[cfg(target_os = "linux")]
const NOFOLLOW_FLAG: i32 = 0x20000;
#[cfg(target_os = "linux")]
const NONBLOCK_FLAG: i32 = 0x800;
#[cfg(target_os = "macos")]
const NOFOLLOW_FLAG: i32 = 0x100;
#[cfg(target_os = "macos")]
const NONBLOCK_FLAG: i32 = 0x4;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const CFG_FIXTURES: &[&str] = &[
    "crates/velnor-actions-mise/tests/impl_mise_git_owned_config_fixture.rs",
    "crates/velnor-actions-mise/tests/impl_mise_git_index_fixture.rs",
    "crates/test_support/git_fixture.rs",
];

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "impl_mise_forbidden_src_inventory_tests.rs"]
mod tests;

/// Load the complete source and explicitly admitted configuration-test closure.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn source_units(src_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let root = repository_root(src_dir)?;
    let mut units = source_files(src_dir)?
        .into_iter()
        .map(|path| {
            let relative = relative_path(&root, &path)?;
            Ok((relative, read_source(&path)?))
        })
        .collect::<Result<Vec<_>, String>>()?;
    for relative in CFG_FIXTURES {
        let path = relative_path_from_root(&root, relative)?;
        units.push(((*relative).to_owned(), read_source(&path)?));
    }
    units.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(units)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn reverse_units(src_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let root = repository_root(src_dir)?;
    let groups = ["crates/velnor-actions-mise/tests", "crates/test_support"]
        .into_iter()
        .map(|relative| {
            let directory = relative_path_from_root(&root, relative)?;
            source_files(&directory)?
                .into_iter()
                .map(|path| Ok((relative_path(&root, &path)?, read_source(&path)?)))
                .collect::<Result<Vec<_>, String>>()
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(groups.into_iter().flatten().collect())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn source_units(_src_dir: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    Err("source inventory unsupported platform".to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn reverse_units(_src_dir: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    Err("source inventory unsupported platform".to_owned())
}

/// Recursively enumerate regular Rust files while rejecting every symlink.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn source_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    checked_ancestry(dir)?;
    directory_identity(dir)?;
    let mut found = Vec::new();
    collect_files(dir, &mut found)?;
    found.sort_unstable();
    Ok(found)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn source_files(_dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    Err("source inventory unsupported platform".to_owned())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn repository_root(src_dir: &Path) -> Result<PathBuf, String> {
    checked_ancestry(src_dir)?;
    if src_dir.file_name() != Some(OsStr::new("src")) {
        return Err(format!(
            "source root is not named src: {}",
            display(src_dir)
        ));
    }
    let crate_dir = src_dir
        .parent()
        .ok_or_else(|| format!("source root has no crate parent: {}", display(src_dir)))?;
    if crate_dir.file_name() != Some(OsStr::new("velnor-actions-mise")) {
        return Err(format!(
            "source root is outside velnor-actions-mise: {}",
            display(src_dir)
        ));
    }
    let crates_dir = crate_dir
        .parent()
        .ok_or_else(|| format!("crate has no crates parent: {}", display(src_dir)))?;
    if crates_dir.file_name() != Some(OsStr::new("crates")) {
        return Err(format!(
            "source root is outside crates: {}",
            display(src_dir)
        ));
    }
    let root = crates_dir
        .parent()
        .ok_or_else(|| {
            format!(
                "crates directory has no repository root: {}",
                display(src_dir)
            )
        })?
        .to_path_buf();
    directory_identity(&root)?;
    directory_identity(crates_dir)?;
    directory_identity(crate_dir)?;
    Ok(root)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn collect_files(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let before = directory_identity(dir)?;
    let mut entries = fs::read_dir(dir)
        .map_err(|error| io_error("read source directory", dir, error))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error("enumerate source directory", dir, error))?;
    entries.sort_unstable();
    for path in entries {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error("inspect source entry", &path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "source inventory rejects symlink: {}",
                display(&path)
            ));
        }
        if metadata.file_type().is_dir() {
            collect_files(&path, found)?;
        } else if metadata.file_type().is_file() {
            regular_identity(&path)?;
            if path.extension() == Some(OsStr::new("rs")) {
                found.push(path);
            }
        } else {
            return Err(format!(
                "source inventory rejects non-regular entry: {}",
                display(&path)
            ));
        }
    }
    let after = directory_identity(dir)?;
    if before != after {
        return Err(format!(
            "source directory changed while reading: {}",
            display(dir)
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn checked_ancestry(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("source path must be absolute: {}", display(path)));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(part) => {
                current.push(part);
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|error| io_error("inspect source ancestry", &current, error))?;
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "source inventory rejects symlink ancestry: {}",
                        display(&current)
                    ));
                }
                if current != path && !metadata.file_type().is_dir() {
                    return Err(format!(
                        "source ancestry is not a directory: {}",
                        display(&current)
                    ));
                }
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(format!(
                    "source path is not lexically closed: {}",
                    display(path)
                ));
            }
        }
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| io_error("canonicalize source path", path, error))?;
    if canonical != path {
        return Err(format!("source path is not canonical: {}", display(path)));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn directory_identity(path: &Path) -> Result<NodeIdentity, String> {
    checked_ancestry(path)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect source directory", path, error))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "source inventory requires a real directory: {}",
            display(path)
        ));
    }
    Ok(NodeIdentity::from_metadata(&metadata))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn regular_identity(path: &Path) -> Result<FileIdentity, String> {
    checked_ancestry(path)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error("inspect source file", path, error))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "source inventory requires a regular file: {}",
            display(path)
        ));
    }
    let identity = FileIdentity::from_metadata(&metadata);
    if identity.nlink != 1 {
        return Err(format!(
            "source inventory rejects hardlink alias: {}",
            display(path)
        ));
    }
    Ok(identity)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_source(path: &Path) -> Result<String, String> {
    let before = regular_identity(path)?;
    if before.size > MAX_SOURCE_BYTES {
        return Err(format!("source_inventory:size_limit: {}", display(path)));
    }
    let mut file = open_read(path)?;
    let held_metadata = file
        .metadata()
        .map_err(|error| io_error("inspect opened source", path, error))?;
    let held = FileIdentity::from_metadata(&held_metadata);
    if held != before || held.nlink != 1 || !held_metadata.file_type().is_file() {
        return Err(format!(
            "source file changed before read: {}",
            display(path)
        ));
    }
    let read_limit = MAX_SOURCE_BYTES
        .checked_add(1)
        .ok_or_else(|| format!("source_inventory:size_limit_overflow: {}", display(path)))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read source", path, error))?;
    let byte_count = u64::try_from(bytes.len())
        .map_err(|_| format!("source_inventory:size_overflow: {}", display(path)))?;
    if byte_count > MAX_SOURCE_BYTES || byte_count != before.size {
        return Err(format!(
            "source_inventory:size_limit_or_changed: {}",
            display(path)
        ));
    }
    let fd_after_metadata = file
        .metadata()
        .map_err(|error| io_error("inspect opened source after read", path, error))?;
    let fd_after = FileIdentity::from_metadata(&fd_after_metadata);
    let path_after = regular_identity(path)?;
    if !fd_after_metadata.file_type().is_file() || fd_after != before || path_after != before {
        return Err(format!("source file changed after read: {}", display(path)));
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("source is not UTF-8 {}: {error}", display(path)))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_read(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW_FLAG | NONBLOCK_FLAG)
        .open(path)
        .map_err(|error| io_error("open source", path, error))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| format!("source path escapes repository root: {error}"))?;
    relative
        .to_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("source path is not UTF-8: {}", display(path)))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn relative_path_from_root(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("fixture path is not lexically closed: {relative}"));
    }
    let path = root.join(relative_path);
    checked_ancestry(&path)?;
    Ok(path)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn io_error(action: &str, path: &Path, error: std::io::Error) -> String {
    format!("{action} {}: {error}", display(path))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NodeIdentity {
    dev: u64,
    ino: u64,
    mode: u32,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl NodeIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: metadata.mode(),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
    nlink: u64,
    mode: u32,
    size: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            nlink: metadata.nlink(),
            mode: metadata.mode(),
            size: metadata.size(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
        }
    }
}
