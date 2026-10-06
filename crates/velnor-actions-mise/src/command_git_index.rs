//! Private Git index preparation for read-only discovery commands.
//!
//! Git may refresh the index while answering a read-only query. This module
//! resolves the repository-owned index, validates its bytes, and gives Git a
//! private copy whose lifetime is tied to the command owner. It does not spawn
//! Git and does not alter the process environment.

#[path = "command_git_index_checksum.rs"]
mod checksum;
#[path = "command_git_index_format.rs"]
mod format;
#[path = "command_git_index_fs.rs"]
pub(super) mod fs;
#[path = "command_git_repository.rs"]
pub(super) mod repository;
#[path = "command_git_index_repository_format.rs"]
pub(super) mod repository_format;

use super::private_root::{BoundCwd, PrivateGitRoot};
use std::io;
use std::path::{Path, PathBuf};

/// Owned private index and its cleanup guard.
#[derive(Debug)]
pub(super) struct PrivateGitIndex {
    root: PrivateGitRoot,
    cwd: BoundCwd,
    index: PathBuf,
    context: repository::RepositoryContext,
    common: repository_format::CommonRepository,
    format: Option<repository_format::IndexObjectFormat>,
    source_snapshot: repository_format::config::ConfigSnapshot,
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

    pub(super) fn root(&self) -> &PrivateGitRoot {
        &self.root
    }

    pub(super) fn common_repository(&self) -> &repository_format::CommonRepository {
        &self.common
    }

    pub(super) fn common_path(&self) -> &Path {
        self.common.path()
    }

    pub(super) fn expected_format(&self) -> Option<repository_format::IndexObjectFormat> {
        self.format
    }

    pub(super) fn verify_source(&self) -> io::Result<()> {
        self.cwd.verify_binding()?;
        self.context.verify_binding()?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.source_snapshot.verify()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(io::Error::other("private_git_index:unsupported_platform"))
        }
    }

    pub(super) fn bound_cwd(&self) -> &BoundCwd {
        &self.cwd
    }

    pub(super) fn context(&self) -> &repository::RepositoryContext {
        &self.context
    }

    /// Remove the private directory and return cleanup failures to the owner.
    pub(super) fn finish(self) -> io::Result<()> {
        self.root.finish()
    }

    /// An un-reaped child may still own this path; retain it on that OS failure.
    pub(super) fn retain_unreaped(self) {
        self.root.retain_unreaped();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn prepare_supported(cwd: Option<&PathBuf>) -> io::Result<PrivateGitIndex> {
    let bound_cwd = BoundCwd::from_owner(cwd)?;
    let context = repository::RepositoryContext::resolve(cwd)?;
    bound_cwd.verify_binding()?;
    if context.cwd() != bound_cwd.path() {
        return Err(io::Error::other("private_git_index:cwd_binding_mismatch"));
    }
    let source = context.source_index();
    let source_snapshot = repository_format::config::snapshot(
        source.to_path_buf(),
        fs::MAX_INDEX_BYTES,
        "source_index",
    )?;
    let index_format = match &source_snapshot.bytes {
        Some(bytes) => {
            let width = format::validate(bytes).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("private_git_index:invalid_format:{error}"),
                )
            })?;
            let index_format = repository_format::IndexObjectFormat::from_width(width)
                .ok_or_else(|| io::Error::other("private_git_index:invalid_oid_width"))?;
            Some(index_format)
        }
        None => None,
    };

    context.verify_binding()?;
    let common = repository_format::CommonRepository::capture(&context)?;
    let root = PrivateGitRoot::create_repository(&context, &common)?;
    let guard = PrivateGitIndex {
        index: root.path().join("index"),
        root,
        cwd: bound_cwd,
        context,
        common,
        format: index_format,
        source_snapshot,
    };
    if let Some(bytes) = &guard.source_snapshot.bytes
        && let Err(error) = fs::write_private_index(&guard.root, bytes)
    {
        return fail_after_create(guard, error);
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
