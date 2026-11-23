// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const KEEP_ENV: &str = "VELNOR_KEEP_TEST_DIRS";

#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        remove_tree(&path).unwrap_or_else(|e| unreachable!("cannot clear {}: {e}", path.display()));
        fs::create_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
        Self { path }
    }

    #[must_use]
    pub fn adopt(path: PathBuf) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn canonicalized(mut self) -> std::io::Result<Self> {
        self.path = fs::canonicalize(&self.path)?;
        Ok(self)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if !keeping() {
            remove_tree(&self.path).ok();
        }
    }
}

pub fn remove_tree(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    if fs::remove_dir_all(path).is_ok() {
        return Ok(());
    }
    make_writable(path)?;
    fs::remove_dir_all(path)
}

fn make_writable(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Ok(());
    }
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o700);
    fs::set_permissions(path, permissions)?;
    for entry in fs::read_dir(path)? {
        make_writable(&entry?.path())?;
    }
    Ok(())
}

impl Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

#[must_use]
pub fn keeping() -> bool {
    keep_requested(std::env::var_os(KEEP_ENV).as_deref())
}

#[must_use]
pub fn keep_requested(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{KEEP_ENV, ScratchDir, keep_requested, keeping};

    const CHILD_PATH_ENV: &str = "VELNOR_SCRATCH_SELFTEST_CHILD_PATH";

    fn entry_count(root: &Path) -> usize {
        fs::read_dir(root).map_or(0, Iterator::count)
    }

    fn fresh_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "velnor-scratch-selftest-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap_or_else(|e| unreachable!("{e}"));
        root
    }

    fn selftest_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "velnor-scratch-selftest-{}-{name}",
            std::process::id()
        ))
    }

    fn lock(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn assert_cleanup_leaves_nothing_behind() {
        let root = fresh_root();
        let retained = usize::from(keeping());

        let success = ScratchDir::new(root.join("success"));
        fs::write(success.join("payload"), "data").unwrap_or_else(|e| unreachable!("{e}"));
        drop(success);
        assert_eq!(entry_count(&root), retained);

        let panicked = catch_unwind(AssertUnwindSafe(|| {
            let _guard = ScratchDir::new(root.join("panic"));
            panic_any("expected test panic");
        }));
        assert!(panicked.is_err());
        assert_eq!(entry_count(&root), 2 * retained);

        fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn scratch_dirs_leave_nothing_in_tmpdir_on_success_or_panic() {
        assert_cleanup_leaves_nothing_behind();
    }

    #[test]
    fn a_stale_tree_at_the_same_path_is_cleared_before_reuse() {
        let path = selftest_path("reuse");
        fs::create_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(path.join("left"), "stale").unwrap_or_else(|e| unreachable!("{e}"));
        let fresh = ScratchDir::new(path.clone());
        assert!(!fresh.join("left").exists());
        drop(fresh);
        assert_eq!(path.exists(), keeping());
    }

    #[test]
    fn a_stale_tree_that_cannot_be_removed_is_cleared_before_reuse() {
        let path = selftest_path("locked");
        let locked = path.join("locked");
        fs::create_dir_all(&locked).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(locked.join("left"), "stale").unwrap_or_else(|e| unreachable!("{e}"));
        lock(&locked, 0o500);
        let fresh = ScratchDir::new(path.clone());
        assert!(!locked.exists());
        drop(fresh);
        assert_eq!(path.exists(), keeping());
    }

    #[test]
    fn a_tree_that_cannot_be_removed_is_cleaned_when_its_guard_drops() {
        let guard = ScratchDir::new(selftest_path("drop-locked"));
        let locked = guard.join("locked");
        fs::create_dir_all(&locked).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(locked.join("left"), "data").unwrap_or_else(|e| unreachable!("{e}"));
        lock(&locked, 0o500);
        let path = guard.path().to_path_buf();
        drop(guard);
        assert_eq!(path.exists(), keeping());
    }

    #[test]
    fn a_kept_directory_survives_its_guard_when_the_flag_is_set() {
        if let Some(path) = std::env::var_os(CHILD_PATH_ENV) {
            drop(ScratchDir::new(PathBuf::from(path)));
            return;
        }
        let path = selftest_path("kept");
        let status = std::process::Command::new(
            std::env::current_exe().unwrap_or_else(|e| unreachable!("{e}")),
        )
        .args([
            "--exact",
            "scratch::tests::a_kept_directory_survives_its_guard_when_the_flag_is_set",
        ])
        .env(KEEP_ENV, "1")
        .env(CHILD_PATH_ENV, &path)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
        let kept = path.exists();
        fs::remove_dir_all(&path).ok();
        assert!(status.success());
        assert!(kept);
    }

    #[test]
    fn keep_is_requested_only_by_the_value_one() {
        assert!(keep_requested(Some(std::ffi::OsStr::new("1"))));
        assert!(!keep_requested(Some(std::ffi::OsStr::new("0"))));
        assert!(!keep_requested(Some(std::ffi::OsStr::new(""))));
        assert!(!keep_requested(None));
    }
}
