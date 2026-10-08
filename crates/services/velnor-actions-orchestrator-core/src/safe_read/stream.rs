//! Descriptor-relative, bounded streaming of one repository file.

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use crate::OrchestratorError;

use super::{unreadable, unsafe_path};

/// Stream a declared repository file through a descriptor-relative walk.
/// Every intermediate component is opened as a non-symlink directory and
/// retained until the leaf is opened. The leaf uses `O_NOFOLLOW|O_NONBLOCK`,
/// is `fstat`-checked as regular, and is read through that handle. The callback
/// receives only bytes within `max_bytes`; an oversize file returns an error
/// after the allowed prefix and must not be treated as a complete result.
///
/// # Errors
/// Returns an IO or unsafe-path error for absent, linked, non-file, escaping,
/// or oversize inputs, and forwards callback errors unchanged.
pub fn stream_repo_file(
    root: &Path,
    rel: &str,
    max_bytes: u64,
    mut on_chunk: impl FnMut(&[u8]) -> Result<(), OrchestratorError>,
) -> Result<u64, OrchestratorError> {
    let parent = open_repo_parent(root, rel)?;
    stream_repo_file_from_parent(&parent, max_bytes, &mut on_chunk)
}

/// Pinned directory chain and final component for one repo-relative file.
/// Keeping every opened directory alive prevents a renamed/replaced pathname
/// from redirecting a later component lookup.
pub(super) struct RepoFileParent {
    display_path: PathBuf,
    directories: Vec<OwnedFd>,
    leaf: String,
}

pub(super) fn open_repo_parent(
    root: &Path,
    rel: &str,
) -> Result<RepoFileParent, OrchestratorError> {
    let display_path = root.join(rel);
    let components = rel.split('/').collect::<Vec<_>>();
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel.chars().any(char::is_control)
        || components
            .iter()
            .any(|part| matches!(*part, "" | "." | ".."))
    {
        return Err(unsafe_path(&display_path, "unsafe_relative_path"));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|err| unreadable(root, err.to_string()))?;
    let root_fd = rustix::fs::open(
        &canonical_root,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|err| unreadable(root, std::io::Error::from(err).to_string()))?;
    let mut directories = vec![root_fd];
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let parent = directories
            .last()
            .ok_or_else(|| unreadable(&display_path, "root_directory_unavailable"))?;
        let directory = rustix::fs::openat(
            parent,
            *component,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|err| repo_component_open_error(&display_path, err))?;
        directories.push(directory);
    }
    let leaf = components
        .last()
        .ok_or_else(|| unsafe_path(&display_path, "unsafe_relative_path"))?
        .to_string();
    Ok(RepoFileParent {
        display_path,
        directories,
        leaf,
    })
}

pub(super) fn stream_repo_file_from_parent(
    parent: &RepoFileParent,
    max_bytes: u64,
    on_chunk: &mut impl FnMut(&[u8]) -> Result<(), OrchestratorError>,
) -> Result<u64, OrchestratorError> {
    let directory = parent
        .directories
        .last()
        .ok_or_else(|| unreadable(&parent.display_path, "root_directory_unavailable"))?;
    let fd = rustix::fs::openat(
        directory,
        parent.leaf.as_str(),
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|err| repo_component_open_error(&parent.display_path, err))?;
    super::stream_open_file(fd, &parent.display_path, max_bytes, None, on_chunk)
}

fn repo_component_open_error(path: &Path, error: rustix::io::Errno) -> OrchestratorError {
    if error == rustix::io::Errno::LOOP {
        unsafe_path(path, "symlink_refused")
    } else if error == rustix::io::Errno::NOENT {
        unreadable(path, "not_found")
    } else if error == rustix::io::Errno::NOTDIR {
        unreadable(path, "not_a_directory")
    } else {
        unreadable(path, std::io::Error::from(error).to_string())
    }
}
