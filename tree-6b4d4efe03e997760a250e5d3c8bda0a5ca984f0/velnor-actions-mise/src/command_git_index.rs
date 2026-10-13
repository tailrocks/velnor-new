//! Private Git index preparation for read-only discovery commands.
//!
//! Git may refresh the index while answering a read-only query. This module
//! resolves the repository-owned index, validates its bytes, and gives Git a
//! private copy whose lifetime is tied to the command owner. It does not spawn
//! Git and does not alter the process environment.

#[path = "command_git_index_format.rs"]
mod format;
#[path = "command_git_index_fs.rs"]
mod fs;

use std::fs as std_fs;
use std::io;
use std::path::{Path, PathBuf};

/// Owned private index and its cleanup guard.
#[derive(Debug)]
pub(super) struct PrivateGitIndex {
    directory: PathBuf,
    index: PathBuf,
    armed: bool,
}

impl PrivateGitIndex {
    /// Resolve, validate, and privately copy the repository index.
    pub(super) fn prepare(cwd: Option<&PathBuf>) -> io::Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            prepare_supported(cwd)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = cwd;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "private_git_index:unsupported_platform",
            ))
        }
    }

    /// Absolute private index path for `GIT_INDEX_FILE` routing by the owner.
    pub(super) fn path(&self) -> &Path {
        &self.index
    }

    /// Remove the private directory and return cleanup failures to the owner.
    pub(super) fn finish(mut self) -> io::Result<()> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> io::Result<()> {
        match std_fs::remove_dir_all(&self.directory) {
            Ok(()) => {
                self.armed = false;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.armed = false;
                Ok(())
            }
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!(
                    "private_git_index:cleanup:{}:{error}",
                    self.directory.display()
                ),
            )),
        }
    }
}

impl Drop for PrivateGitIndex {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Err(error) = self.cleanup() {
            eprintln!("private_git_index:cleanup_failed:{error}");
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn prepare_supported(cwd: Option<&PathBuf>) -> io::Result<PrivateGitIndex> {
    let effective_cwd = effective_cwd(cwd)?;
    let source = source_index(&effective_cwd)?;
    let bytes = match fs::read_checked(&source, fs::MAX_INDEX_BYTES, "source_index") {
        Ok(bytes) => {
            format::validate(&bytes).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("private_git_index:invalid_format:{error}"),
                )
            })?;
            Some(bytes)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let directory = fs::create_private_directory()?;
    let guard = PrivateGitIndex {
        index: directory.join("index"),
        directory,
        armed: true,
    };
    if let Some(bytes) = bytes {
        if let Err(error) = fs::write_private_index(&guard.directory, &bytes) {
            return fail_after_create(guard, error);
        }
    }
    Ok(guard)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn fail_after_create(guard: PrivateGitIndex, error: io::Error) -> io::Result<PrivateGitIndex> {
    match guard.finish() {
        Ok(()) => Err(error),
        Err(cleanup) => {
            eprintln!("private_git_index:cleanup_after_error:{cleanup}");
            Err(error)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn effective_cwd(cwd: Option<&PathBuf>) -> io::Result<PathBuf> {
    let raw = match cwd {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => std::env::current_dir()
            .map_err(|error| with_path(error, "current_dir", path))?
            .join(path),
        None => std::env::current_dir().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("private_git_index:current_dir:{error}"),
            )
        })?,
    };
    raw.canonicalize()
        .map_err(|error| with_path(error, "cwd", &raw))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn source_index(cwd: &Path) -> io::Result<PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        let marker = current.join(".git");
        match std_fs::symlink_metadata(&marker) {
            Ok(metadata) => return marker_source(&current, &marker, &metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(with_path(error, "git_marker", &marker)),
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    bare_source(cwd)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn marker_source(root: &Path, marker: &Path, metadata: &std_fs::Metadata) -> io::Result<PathBuf> {
    if metadata.file_type().is_symlink() {
        return Err(invalid("git_marker", "symlink"));
    }
    if metadata.file_type().is_dir() {
        return Ok(marker.join("index"));
    }
    if metadata.file_type().is_file() {
        let gitdir = pointer_target(root, marker)?;
        return Ok(gitdir.join("index"));
    }
    Err(invalid("git_marker", "invalid_type"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pointer_target(root: &Path, marker: &Path) -> io::Result<PathBuf> {
    let bytes = fs::read_checked(marker, fs::MAX_POINTER_BYTES, "git_pointer")?;
    let text = String::from_utf8(bytes).map_err(|_| invalid("git_pointer", "invalid_utf8"))?;
    let Some(mut line) = text.strip_prefix("gitdir: ") else {
        return Err(invalid("git_pointer", "invalid_prefix"));
    };
    if line.ends_with('\n') {
        line = &line[..line.len() - 1];
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
    }
    if line.is_empty() || line.contains(['\r', '\n', '\0']) {
        return Err(invalid("git_pointer", "invalid_target"));
    }
    let unresolved = PathBuf::from(line);
    let unresolved = if unresolved.is_absolute() {
        unresolved
    } else {
        root.join(unresolved)
    };
    let metadata = std_fs::symlink_metadata(&unresolved)
        .map_err(|error| with_path(error, "git_pointer_target", &unresolved))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(invalid("git_pointer_target", "invalid_directory"));
    }
    let canonical = unresolved
        .canonicalize()
        .map_err(|error| with_path(error, "git_pointer_target", &unresolved))?;
    let canonical_metadata = std_fs::symlink_metadata(&canonical)
        .map_err(|error| with_path(error, "git_pointer_target", &canonical))?;
    if canonical_metadata.file_type().is_symlink() || !canonical_metadata.file_type().is_dir() {
        return Err(invalid("git_pointer_target", "invalid_directory"));
    }
    Ok(canonical)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn bare_source(cwd: &Path) -> io::Result<PathBuf> {
    let head = fs::normal_file(&cwd.join("HEAD"), "bare_head")?;
    let objects = fs::normal_directory(&cwd.join("objects"), "bare_objects")?;
    if head && objects {
        return Ok(cwd.join("index"));
    }
    if !head && !objects {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "private_git_index:repository_not_found",
        ));
    }
    Err(invalid("bare_repository", "incomplete_layout"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn invalid(label: &str, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("private_git_index:{label}:{detail}"),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn with_path(error: io::Error, label: &str, path: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("private_git_index:{label}:{}:{error}", path.display()),
    )
}
