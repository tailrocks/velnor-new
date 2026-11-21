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

    use super::{ScratchDir, keep_requested};

    fn leftovers(root: &Path, prefix: &str) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(prefix))
            })
            .collect()
    }

    #[test]
    fn scratch_dirs_leave_nothing_in_tmpdir_on_success_or_panic() {
        let root = std::env::temp_dir();
        let prefix = format!("velnor-scratch-selftest-{}", std::process::id());
        let before = leftovers(&root, &prefix);

        let success = ScratchDir::new(root.join(format!("{prefix}-success")));
        fs::write(success.join("payload"), "data").unwrap_or_else(|e| unreachable!("{e}"));
        drop(success);

        let panicked = catch_unwind(AssertUnwindSafe(|| {
            let _guard = ScratchDir::new(root.join(format!("{prefix}-panic")));
            panic_any("expected test panic");
        }));
        assert!(panicked.is_err());

        assert_eq!(leftovers(&root, &prefix), before);
    }

    #[test]
    fn keep_is_requested_only_by_the_value_one() {
        assert!(keep_requested(Some(std::ffi::OsStr::new("1"))));
        assert!(!keep_requested(Some(std::ffi::OsStr::new("0"))));
        assert!(!keep_requested(Some(std::ffi::OsStr::new(""))));
        assert!(!keep_requested(None));
    }
}
