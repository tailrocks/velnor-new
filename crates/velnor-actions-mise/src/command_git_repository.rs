//! Pinned repository routing for read-only Git discovery.
//!
//! The context is resolved once, from a canonical working directory, and is
//! then applied to the child command by the owner. Callers cannot supply Git
//! routing variables through this module.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "command_git_repository_errors.rs"]
mod errors;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use self::errors::unsupported;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use self::errors::{invalid, with_path};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs as std_fs;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::MetadataExt;

/// Repository paths and identities pinned for one Git child.
#[derive(Debug)]
pub(in crate::command::git) struct RepositoryContext {
    cwd: PathBuf,
    gitdir: PathBuf,
    worktree: Option<PathBuf>,
    source_index: PathBuf,
    cwd_binding: DirectoryBinding,
    gitdir_binding: DirectoryBinding,
    worktree_binding: Option<DirectoryBinding>,
    pointer: Option<PointerBinding>,
}

impl RepositoryContext {
    /// Resolve and pin repository routing without consulting Git or ambient
    /// `GIT_*` selectors.
    pub(in crate::command::git) fn resolve(cwd: Option<&PathBuf>) -> io::Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            resolve_supported(cwd)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = cwd;
            Err(unsupported())
        }
    }

    /// Canonical effective working directory.
    pub(in crate::command::git) fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Canonical Git metadata directory.
    pub(in crate::command::git) fn gitdir(&self) -> &Path {
        &self.gitdir
    }

    /// Canonical worktree root, or `None` for a bare repository.
    pub(in crate::command::git) fn worktree(&self) -> Option<&Path> {
        self.worktree.as_deref()
    }

    /// Repository-owned source index path.
    pub(in crate::command::git) fn source_index(&self) -> &Path {
        &self.source_index
    }

    /// Verify that the pinned routing objects still identify the same paths.
    pub(in crate::command::git) fn verify_binding(&self) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            verify_directory(&self.cwd, &self.cwd_binding, "cwd")?;
            verify_directory(&self.gitdir, &self.gitdir_binding, "gitdir")?;
            if let (Some(worktree), Some(binding)) = (&self.worktree, &self.worktree_binding) {
                verify_directory(worktree, binding, "worktree")?;
            }
            if let Some(pointer) = &self.pointer {
                let bytes =
                    fs::read_checked(&pointer.path, fs::MAX_POINTER_BYTES, "git_pointer_verify")?;
                if bytes != pointer.bytes {
                    return Err(invalid("git_pointer_verify", "changed"));
                }
                let stamp = pointer_binding(&pointer.path)?;
                if stamp != pointer.stamp {
                    return Err(invalid("git_pointer_verify", "metadata_changed"));
                }
            }
            Ok(())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(unsupported())
        }
    }

    /// Apply only the internally resolved Git routing to a child command.
    pub(in crate::command::git) fn apply(&self, command: &mut Command) -> io::Result<()> {
        self.verify_binding()?;
        command.current_dir(&self.cwd);
        command.env("GIT_DIR", &self.gitdir);
        match &self.worktree {
            Some(worktree) => command.env("GIT_WORK_TREE", worktree),
            None => command.env_remove("GIT_WORK_TREE"),
        };
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryBinding {
    dev: u64,
    ino: u64,
    uid: u32,
    mode: u32,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryBinding;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, Eq, PartialEq)]
struct PointerBinding {
    path: PathBuf,
    bytes: Vec<u8>,
    stamp: FileBinding,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Debug)]
struct PointerBinding;

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
fn resolve_supported(cwd: Option<&PathBuf>) -> io::Result<RepositoryContext> {
    let cwd = effective_cwd(cwd)?;
    let cwd_binding = directory_binding(&cwd, "cwd")?;
    let mut current = cwd.clone();
    loop {
        let marker = current.join(".git");
        match std_fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                let (gitdir, gitdir_binding) = canonical_directory(&marker, "gitdir")?;
                let worktree_binding = directory_binding(&current, "worktree")?;
                return Ok(context(
                    cwd,
                    cwd_binding,
                    gitdir,
                    gitdir_binding,
                    Some(current),
                    Some(worktree_binding),
                    None,
                ));
            }
            Ok(metadata) if metadata.file_type().is_file() => {
                let (gitdir, gitdir_binding, pointer) = pointer_target(&current, &marker)?;
                let worktree_binding = directory_binding(&current, "worktree")?;
                return Ok(context(
                    cwd,
                    cwd_binding,
                    gitdir,
                    gitdir_binding,
                    Some(current),
                    Some(worktree_binding),
                    Some(pointer),
                ));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid("git_marker", "symlink"));
            }
            Ok(_) => return Err(invalid("git_marker", "invalid_type")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(with_path(&error, "git_marker", &marker)),
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    resolve_bare(cwd, cwd_binding)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn context(
    cwd: PathBuf,
    cwd_binding: DirectoryBinding,
    gitdir: PathBuf,
    gitdir_binding: DirectoryBinding,
    worktree: Option<PathBuf>,
    worktree_binding: Option<DirectoryBinding>,
    pointer: Option<PointerBinding>,
) -> RepositoryContext {
    let source_index = gitdir.join("index");
    RepositoryContext {
        cwd,
        gitdir,
        worktree,
        source_index,
        cwd_binding,
        gitdir_binding,
        worktree_binding,
        pointer,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn resolve_bare(cwd: PathBuf, cwd_binding: DirectoryBinding) -> io::Result<RepositoryContext> {
    let head = fs::normal_file(&cwd.join("HEAD"), "bare_head")?;
    let objects = fs::normal_directory(&cwd.join("objects"), "bare_objects")?;
    if !head && !objects {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "private_git_repository:repository_not_found",
        ));
    }
    if !head || !objects {
        return Err(invalid("bare_repository", "incomplete_layout"));
    }
    let (gitdir, gitdir_binding) = canonical_directory(&cwd, "bare_gitdir")?;
    Ok(context(
        cwd,
        cwd_binding,
        gitdir,
        gitdir_binding,
        None,
        None,
        None,
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn effective_cwd(cwd: Option<&PathBuf>) -> io::Result<PathBuf> {
    let base = std::env::current_dir()
        .map_err(|error| with_path(&error, "current_dir", Path::new(".")))?;
    let raw = match cwd {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => base.join(path),
        None => base,
    };
    canonical_directory(&raw, "cwd").map(|(path, _)| path)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn canonical_directory(path: &Path, label: &str) -> io::Result<(PathBuf, DirectoryBinding)> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|error| with_path(&error, label, path))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid(label, "symlink"));
    }
    if !metadata.file_type().is_dir() {
        return Err(invalid(label, "not_directory"));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| with_path(&error, label, path))?;
    let binding = directory_binding(&canonical, label)?;
    let path_binding = directory_metadata(&metadata, label)?;
    if binding != path_binding {
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
    if metadata.file_type().is_symlink() {
        return Err(invalid(label, "symlink"));
    }
    if !metadata.file_type().is_dir() {
        return Err(invalid(label, "not_directory"));
    }
    Ok(DirectoryBinding {
        dev: metadata.dev(),
        ino: metadata.ino(),
        uid: metadata.uid(),
        mode: metadata.mode() & 0o7777,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_directory(path: &Path, expected: &DirectoryBinding, label: &str) -> io::Result<()> {
    let actual = directory_binding(path, label)?;
    if &actual == expected {
        Ok(())
    } else {
        Err(invalid(label, "binding_changed"))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pointer_target(
    root: &Path,
    marker: &Path,
) -> io::Result<(PathBuf, DirectoryBinding, PointerBinding)> {
    let before = pointer_binding(marker)?;
    let bytes = fs::read_checked(marker, fs::MAX_POINTER_BYTES, "git_pointer")?;
    let after = pointer_binding(marker)?;
    if before != after {
        return Err(invalid("git_pointer", "changed_during_read"));
    }
    let text = String::from_utf8(bytes.clone()).map_err(|_| invalid("git_pointer", "utf8"))?;
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
    let (gitdir, binding) = canonical_directory(&unresolved, "git_pointer_target")?;
    Ok((
        gitdir,
        binding,
        PointerBinding {
            path: marker.to_path_buf(),
            bytes,
            stamp: after,
        },
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pointer_binding(path: &Path) -> io::Result<FileBinding> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|error| with_path(&error, "git_pointer", path))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(invalid("git_pointer", "not_regular_file"));
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
    if binding.nlink != 1 {
        return Err(invalid("git_pointer", "inode_alias"));
    }
    Ok(binding)
}
