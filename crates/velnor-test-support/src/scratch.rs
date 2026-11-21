// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

const KEEP_ENV: &str = "VELNOR_KEEP_TEST_DIRS";

#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        fs::remove_dir_all(&path).ok();
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
        if !keep_requested(std::env::var_os(KEEP_ENV).as_deref()) {
            fs::remove_dir_all(&self.path).ok();
        }
    }
}

impl Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

#[must_use]
pub fn keep_requested(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{ScratchDir, keep_requested};

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

    fn assert_cleanup_leaves_nothing_behind() {
        let root = fresh_root();

        let success = ScratchDir::new(root.join("success"));
        fs::write(success.join("payload"), "data").unwrap_or_else(|e| unreachable!("{e}"));
        drop(success);
        assert_eq!(entry_count(&root), 0);

        let panicked = catch_unwind(AssertUnwindSafe(|| {
            let _guard = ScratchDir::new(root.join("panic"));
            panic_any("expected test panic");
        }));
        assert!(panicked.is_err());
        assert_eq!(entry_count(&root), 0);

        fs::remove_dir(&root).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn scratch_dirs_leave_nothing_in_tmpdir_on_success_or_panic() {
        assert_cleanup_leaves_nothing_behind();
    }

    #[test]
    fn a_stale_tree_left_by_an_earlier_process_does_not_break_cleanup() {
        let stale = ScratchDir::adopt(std::env::temp_dir().join(format!(
            "velnor-scratch-selftest-{}-stale-success",
            std::process::id()
        )));
        fs::create_dir_all(stale.path()).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(stale.join("left"), "stale").unwrap_or_else(|e| unreachable!("{e}"));
        assert_cleanup_leaves_nothing_behind();
    }

    #[test]
    fn keep_is_requested_only_by_the_value_one() {
        assert!(keep_requested(Some(std::ffi::OsStr::new("1"))));
        assert!(!keep_requested(Some(std::ffi::OsStr::new("0"))));
        assert!(!keep_requested(Some(std::ffi::OsStr::new(""))));
        assert!(!keep_requested(None));
    }
}
