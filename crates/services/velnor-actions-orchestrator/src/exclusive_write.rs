//! Exclusive no-follow writes for event-time producers.
//!
//! Report, request, and plan-artifact paths are predictable
//! (`$RUNNER_TEMP/velnor/<run-key>/...`), and repository code (build
//! scripts, custom tasks) runs as the same user earlier in the job, so
//! a planted symlink at a write path would redirect producer bytes.
//! `O_EXCL` alone does not stop that: creating through a dangling final
//! symlink still follows it. Files therefore open relative to the
//! parent dirfd with `O_NOFOLLOW|O_EXCL`, and parent chains are
//! created per component under a caller-supplied anchor with a
//! symlink check before and after each step. F6: a same-user attacker
//! can still race any check-use window (parent swap between the pin
//! and the open, swap after verification); this closes the
//! deterministic plant, not a winning race. That residual is accepted:
//! same-user, same-machine TOCTOU has no portable close (no `openat2`
//! on macOS, no atomic pin-then-resolve in std).

use std::fs;
use std::io::Write;
use std::path::{Component, Path};

#[cfg(unix)]
use rustix::fs::{Mode, OFlags, open, openat};

use crate::OrchestratorError;
use crate::internal::internal;

/// Exclusively write one producer file, refusing symlinks.
///
/// A pre-existing file errors with `{context}_exists`, an unwritable
/// file with `{context}_unwritable`, and any symlink at the final
/// component (before or after creation) with `symlink_refused`, never
/// followed.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for symlinks, pre-existing
/// files, and unwritable paths.
pub(crate) fn write_exclusive(
    path: &Path,
    bytes: &[u8],
    context: &str,
) -> Result<(), OrchestratorError> {
    write_exclusive_until(path, bytes, context, || Ok(()))
}

/// Exclusively write in bounded chunks, checking the owning operation's deadline.
pub(crate) fn write_exclusive_until(
    path: &Path,
    bytes: &[u8],
    context: &str,
    mut checkpoint: impl FnMut() -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    checkpoint()?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(internal("symlink_refused")),
        Ok(_) => return Err(internal(&format!("{context}_exists"))),
        Err(_) => {}
    }
    let mut file = open_exclusive(path, context)?;
    if !created_identity(&file, path) {
        return Err(internal("symlink_refused"));
    }
    for chunk in bytes.chunks(64 * 1024) {
        checkpoint()?;
        file.write_all(chunk)
            .map_err(|_| internal(&format!("{context}_unwritable")))?;
        checkpoint()?;
    }
    Ok(())
}

/// Open one new file relative to its parent dirfd, never following symlinks.
///
/// The parent opens with `O_DIRECTORY|O_NOFOLLOW` and the child creates
/// through `openat` with a bare file name
/// (`O_CREAT|O_EXCL|O_WRONLY|O_NOFOLLOW`), so no component is
/// re-resolved after the parent is pinned: a symlink at the final
/// component fails with `ELOOP`, a pre-existing file with `EEXIST`.
/// Components above the immediate parent were pinned by
/// [`create_dir_no_symlink`] at the call site, not here.
#[cfg(unix)]
fn open_exclusive(path: &Path, context: &str) -> Result<std::fs::File, OrchestratorError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(Path::new)
        .ok_or_else(|| internal(&format!("{context}_unwritable")))?;
    let parent_fd = open(
        parent,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| parent_open_error(parent, context))?;
    let created = openat(
        &parent_fd,
        file_name,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        file_mode(),
    )
    .map_err(|err| child_open_error(err, context))?;
    Ok(std::fs::File::from(created))
}

/// Portable fallback: exclusive create without dirfd pinning (non-unix).
#[cfg(not(unix))]
fn open_exclusive(path: &Path, context: &str) -> Result<std::fs::File, OrchestratorError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| internal(&format!("{context}_exists")))
}

/// Creation mode for producer files: `0666` under the umask, like std.
#[cfg(unix)]
fn file_mode() -> Mode {
    Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH
}

/// Parent-open failure: a symlinked parent refuses, anything else is unwritable.
#[cfg(unix)]
fn parent_open_error(parent: &Path, context: &str) -> OrchestratorError {
    if fs::symlink_metadata(parent).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return internal("symlink_refused");
    }
    internal(&format!("{context}_unwritable"))
}

/// Child-open failure: `EEXIST` exists, `ELOOP` refused, else unwritable.
#[cfg(unix)]
fn child_open_error(err: rustix::io::Errno, context: &str) -> OrchestratorError {
    if err == rustix::io::Errno::EXIST {
        return internal(&format!("{context}_exists"));
    }
    if err == rustix::io::Errno::LOOP {
        return internal("symlink_refused");
    }
    internal(&format!("{context}_unwritable"))
}

/// Create one producer directory, refusing symlinked components.
///
/// Every component from `anchor` (exclusive) through `dir` (inclusive)
/// is created one at a time with a symlink check before and after each
/// step; a symlink anywhere in that span refuses with `symlink_refused`
/// instead of writing through it. `dir` must sit under `anchor` with
/// only normal components (`..` after the anchor is `anchor_escape`).
/// `EEXIST` on creation retries as success (concurrent producers), and
/// a file in the way fails closed with an IO error on the next step.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for symlinks and anchor
/// escapes; [`OrchestratorError::Io`] for mkdir failures.
pub(crate) fn create_dir_no_symlink(anchor: &Path, dir: &Path) -> Result<(), OrchestratorError> {
    let relative = dir
        .strip_prefix(anchor)
        .map_err(|_| internal("anchor_escape"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(internal("anchor_escape"));
        };
        parts.push(part);
    }
    let mut current = anchor.to_path_buf();
    for part in parts {
        current.push(part);
        refuse_if_link(&current)?;
        if !current.is_dir() {
            match fs::create_dir(&current) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => {
                    return Err(OrchestratorError::io(
                        current.display().to_string(),
                        err.to_string(),
                    ));
                }
            }
        }
        // F6: the second check catches a link planted between the first
        // check and creation; a swap after this check still wins
        // (accepted same-user race, see the module docs).
        refuse_if_link(&current)?;
    }
    Ok(())
}

/// Refuse one path that resolves to a symlink.
fn refuse_if_link(path: &Path) -> Result<(), OrchestratorError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(internal("symlink_refused"));
    }
    Ok(())
}

/// True when the open handle is the file now at `path`.
///
/// The final component must not be a symlink, and the open file's
/// identity must match the path's, so a swap between creation and
/// verification fails closed instead of writing through the plant.
#[cfg(unix)]
fn created_identity(file: &fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return false;
    }
    match (file.metadata(), fs::metadata(path)) {
        (Ok(open), Ok(at_path)) => open.dev() == at_path.dev() && open.ino() == at_path.ino(),
        _ => false,
    }
}

/// True when the final component is not a symlink (non-unix fallback).
#[cfg(not(unix))]
fn created_identity(_file: &fs::File, path: &Path) -> bool {
    !fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}
#[cfg(test)]
mod tests;
