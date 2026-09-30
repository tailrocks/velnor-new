//! Exclusive no-follow writes for event-time producers.
//!
//! Report, request, and plan-artifact paths are predictable
//! (`$RUNNER_TEMP/velnor/<run-key>/...`), and repository code (build
//! scripts, custom tasks) runs as the same user earlier in the job, so
//! a planted symlink at a write path would redirect producer bytes.
//! `O_EXCL` alone does not stop that: creating through a dangling final
//! symlink still follows it. These writers reject symlinks at the final
//! component before creation and verify the open file is the path that
//! was created (dev/ino identity on unix). A same-user attacker can
//! still race the check-use window; this closes the deterministic
//! plant, not a winning race.

use std::fs;
use std::io::Write;
use std::path::Path;

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
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(internal("symlink_refused")),
        Ok(_) => return Err(internal(&format!("{context}_exists"))),
        Err(_) => {}
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| internal(&format!("{context}_exists")))?;
    if !created_identity(&file, path) {
        return Err(internal("symlink_refused"));
    }
    file.write_all(bytes)
        .map_err(|_| internal(&format!("{context}_unwritable")))
}

/// Create one producer directory, refusing symlinked components.
///
/// Every component from `anchor` (exclusive) through `dir` (inclusive)
/// must exist as a real directory after creation; a symlink anywhere
/// in that span refuses with `symlink_refused` instead of writing
/// through it. `dir` must sit under `anchor`.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for symlinks and paths
/// outside the anchor; [`OrchestratorError::Io`] for mkdir failures.
pub(crate) fn create_dir_no_symlink(anchor: &Path, dir: &Path) -> Result<(), OrchestratorError> {
    fs::create_dir_all(dir)
        .map_err(|err| OrchestratorError::io(dir.display().to_string(), err.to_string()))?;
    let relative = dir
        .strip_prefix(anchor)
        .map_err(|_| internal("symlink_refused"))?;
    let mut current = anchor.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(internal("symlink_refused"));
        }
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
mod tests {
    use super::*;

    /// Scratch dir plus a display-safe join.
    fn scratch(name: &str) -> tempfile::TempDir {
        tempfile::TempDir::with_prefix(format!("exclusive-{name}-")).expect("tempdir")
    }

    #[test]
    fn exclusive_write_round_trips_and_refuses_rewrite() {
        let temp = scratch("roundtrip");
        let file = temp.path().join("report.json");
        write_exclusive(&file, b"{}", "report").expect("first write");
        assert_eq!(fs::read(&file).expect("readback"), b"{}");
        let err = write_exclusive(&file, b"{}", "report").expect_err("rewrite refused");
        assert!(err.to_string().contains("report_exists"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn exclusive_write_refuses_planted_symlinks() {
        let temp = scratch("symlink");
        let target = temp.path().join("target.json");
        let link = temp.path().join("report.json");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = write_exclusive(&link, b"{}", "report").expect_err("link refused");
        assert!(err.to_string().contains("symlink_refused"), "{err}");
        assert!(!target.exists(), "bytes never followed the plant");
    }

    #[cfg(unix)]
    #[test]
    fn dir_creation_refuses_symlinked_components() {
        let temp = scratch("dirlink");
        let real = temp.path().join("real");
        fs::create_dir(&real).expect("real dir");
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let err = create_dir_no_symlink(temp.path(), &link.join("sub"))
            .expect_err("linked parent refused");
        assert!(err.to_string().contains("symlink_refused"), "{err}");
        let clean = temp.path().join("a").join("b");
        create_dir_no_symlink(temp.path(), &clean).expect("clean parents pass");
        assert!(clean.is_dir());
    }
}
