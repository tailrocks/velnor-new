//! Bounded snapshots for the repository format owner's fixed config paths.
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::super::fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs as std_fs;
use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) struct ConfigSnapshot {
    path: PathBuf,
    pub(in crate::command::git) bytes: Option<Vec<u8>>,
    stamp: Option<FileBinding>,
    limit: u64,
    label: &'static str,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl std::fmt::Debug for ConfigSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigSnapshot")
            .field("label", &self.label)
            .field("byte_count", &self.bytes.as_ref().map(Vec::len))
            .finish_non_exhaustive()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Debug)]
pub(in crate::command::git) struct ConfigSnapshot;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) fn snapshot(
    path: PathBuf,
    limit: u64,
    label: &'static str,
) -> io::Result<ConfigSnapshot> {
    match std_fs::symlink_metadata(&path) {
        Ok(_) => {
            let bytes = fs::read_checked(&path, limit, label)?;
            let stamp = file_binding(&path, label)?;
            Ok(ConfigSnapshot {
                path,
                bytes: Some(bytes),
                stamp: Some(stamp),
                limit,
                label,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ConfigSnapshot {
            path,
            bytes: None,
            stamp: None,
            limit,
            label,
        }),
        Err(error) => Err(with_path(&error, label, &path)),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ConfigSnapshot {
    pub(in crate::command::git) fn verify(&self) -> io::Result<()> {
        match &self.bytes {
            None => match std_fs::symlink_metadata(&self.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Ok(_) => Err(invalid(self.label, "appeared")),
                Err(error) => Err(with_path(&error, self.label, &self.path)),
            },
            Some(expected) => {
                let actual = fs::read_checked(&self.path, self.limit, self.label)
                    .map_err(|error| changed(&error, self.label))?;
                if &actual != expected {
                    return Err(invalid(self.label, "changed"));
                }
                let stamp = file_binding(&self.path, self.label)?;
                if Some(stamp) != self.stamp {
                    return Err(invalid(self.label, "metadata_changed"));
                }
                Ok(())
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::command::git) struct DirectoryBinding {
    dev: u64,
    ino: u64,
    uid: u32,
    mode: u32,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::command::git) struct DirectoryBinding;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileBinding {
    dev: u64,
    ino: u64,
    nlink: u64,
    uid: u32,
    mode: u32,
    size: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) fn canonical_directory(
    path: &Path,
    label: &str,
) -> io::Result<(PathBuf, DirectoryBinding)> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|error| with_path(&error, label, path))?;
    let initial = directory_metadata(&metadata, label)?;
    let canonical = path
        .canonicalize()
        .map_err(|error| with_path(&error, label, path))?;
    let binding = directory_binding(&canonical, label)?;
    if binding != initial {
        return Err(invalid(label, "alias"));
    }
    Ok((canonical, binding))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn directory_binding(path: &Path, label: &str) -> io::Result<DirectoryBinding> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|error| with_path(&error, label, path))?;
    directory_metadata(&metadata, label)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn directory_metadata(metadata: &std_fs::Metadata, label: &str) -> io::Result<DirectoryBinding> {
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(invalid(label, "not_normal_directory"));
    }
    Ok(DirectoryBinding {
        dev: metadata.dev(),
        ino: metadata.ino(),
        uid: metadata.uid(),
        mode: metadata.mode() & 0o7777,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::command::git) fn verify_directory(
    path: &Path,
    expected: &DirectoryBinding,
    label: &str,
) -> io::Result<()> {
    if directory_binding(path, label)? == *expected {
        Ok(())
    } else {
        Err(invalid(label, "binding_changed"))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn file_binding(path: &Path, label: &str) -> io::Result<FileBinding> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|error| with_path(&error, label, path))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(invalid(label, "not_normal_file"));
    }
    let binding = FileBinding {
        dev: metadata.dev(),
        ino: metadata.ino(),
        nlink: metadata.nlink(),
        uid: metadata.uid(),
        mode: metadata.mode() & 0o7777,
        size: metadata.size(),
        mtime: metadata.mtime(),
        mtime_nsec: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_nsec: metadata.ctime_nsec(),
    };
    if binding.nlink == 1 {
        Ok(binding)
    } else {
        Err(invalid(label, "inode_alias"))
    }
}

fn changed(error: &io::Error, label: &str) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("private_git_repository_format:{label}:changed:{error}"),
    )
}

pub(in crate::command::git) fn invalid(label: &str, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("private_git_repository_format:{label}:{detail}"),
    )
}

pub(in crate::command::git) fn with_path(error: &io::Error, label: &str, path: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "private_git_repository_format:{label}:{}:{error}",
            path.display()
        ),
    )
}
