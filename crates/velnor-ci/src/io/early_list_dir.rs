// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const PREFIX: &str = "velnor-nextest-list";

pub(super) struct ListDir(PathBuf);

impl ListDir {
    pub(super) fn create(runner_temp: Option<OsString>, process: u32) -> Result<Self, String> {
        let temp = runner_temp
            .filter(|temp| !temp.is_empty())
            .ok_or("RUNNER_TEMP is not set")?;
        let directory = PathBuf::from(temp).join(format!("{PREFIX}-{process}"));
        match fs::remove_dir_all(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot clear {}: {error}", directory.display())),
        }
        fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        Ok(Self(directory))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ListDir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::ListDir;
    use std::ffi::OsString;

    #[test]
    fn creates_a_directory_under_the_runner_temp_and_removes_it_on_drop() {
        let temp = std::env::temp_dir().join(format!("velnor-list-dir-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let directory =
            ListDir::create(Some(OsString::from(&temp)), 7).unwrap_or_else(|e| unreachable!("{e}"));
        let path = directory.path().to_path_buf();
        assert_eq!(path, temp.join("velnor-nextest-list-7"));
        assert!(path.is_dir());
        drop(directory);
        assert!(!path.exists());
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn starts_from_an_empty_directory_when_an_earlier_one_was_left() {
        let temp = std::env::temp_dir().join(format!("velnor-list-stale-{}", std::process::id()));
        let stale = temp.join("velnor-nextest-list-9");
        std::fs::create_dir_all(&stale).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(stale.join("old"), "x").unwrap_or_else(|e| unreachable!("{e}"));
        let directory =
            ListDir::create(Some(OsString::from(&temp)), 9).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(!directory.path().join("old").exists());
        drop(directory);
        std::fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn refuses_a_missing_or_empty_runner_temp() {
        assert!(ListDir::create(None, 1).is_err());
        assert!(ListDir::create(Some(OsString::new()), 1).is_err());
    }
}
