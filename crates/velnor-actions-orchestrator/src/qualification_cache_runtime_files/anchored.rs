//! This module reads files from the directory that it opens for one qualification run.

use std::fs;
use std::io::Read;
use std::path::{Component, Path};

use rustix::fs::{AtFlags, FileType, Mode, OFlags, openat, statat};

use crate::OrchestratorError;
use crate::internal::internal;

pub(in crate::qualification_cache_runtime) struct RunDirectory {
    descriptor: fs::File,
}

impl RunDirectory {
    pub(in crate::qualification_cache_runtime) fn open(
        runner_temp: &Path,
        run_key: &str,
    ) -> Result<Self, OrchestratorError> {
        velnor_actions_contract::validate_run_key(run_key)
            .map_err(crate::internal::internal_contract)?;
        if !runner_temp.is_absolute()
            || runner_temp
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(internal("qualification_cache_runner_temp_path"));
        }
        let metadata = std::fs::symlink_metadata(runner_temp)
            .map_err(|_| internal("qualification_cache_runner_temp_missing"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(internal("qualification_cache_runner_temp_untrusted"));
        }
        let runner_temp = runner_temp
            .canonicalize()
            .map_err(|_| internal("qualification_cache_runner_temp_untrusted"))?;
        let mut current = open_directory(Path::new("/"), None)?;
        for component in runner_temp.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    current = open_directory(Path::new(name), Some(&current))?;
                }
                Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                    return Err(internal("qualification_cache_runner_temp_path"));
                }
            }
        }
        check_owned_directory(&current, "qualification_cache_runner_temp_untrusted")?;

        let mut current = open_directory(Path::new("velnor"), Some(&current))?;
        check_owned_directory(&current, "qualification_cache_plan_directory_untrusted")?;
        current = open_directory(Path::new(run_key), Some(&current))?;
        check_owned_directory(&current, "qualification_cache_plan_directory_untrusted")?;
        Ok(Self {
            descriptor: current,
        })
    }

    pub(in crate::qualification_cache_runtime) fn read_owned_file(
        &self,
        name: &str,
        limit: usize,
        required_mode: Option<u16>,
        prefix: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        let file = open_file(&self.descriptor, name, prefix)?;
        let stat =
            rustix::fs::fstat(&file).map_err(|_| internal(&format!("{prefix}_unreadable")))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_uid != crate::filesystem_owner::effective_uid()
            || required_mode.is_some_and(|mode| stat.st_mode & 0o7777 != mode)
        {
            return Err(internal(&format!("{prefix}_untrusted")));
        }
        let limit = u64::try_from(limit).map_err(|_| internal(&format!("{prefix}_limit")))?;
        let mut reader = fs::File::from(file).take(limit.saturating_add(1));
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|_| internal(&format!("{prefix}_unreadable")))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
            return Err(internal(&format!("{prefix}_oversize")));
        }
        Ok(bytes)
    }

    pub(super) fn file_exists(&self, name: &str) -> Result<bool, OrchestratorError> {
        match statat(&self.descriptor, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => Ok(true),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(_) => Err(internal("qualification_cache_file_unreadable")),
        }
    }
}

fn open_directory(name: &Path, parent: Option<&fs::File>) -> Result<fs::File, OrchestratorError> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let descriptor = match parent {
        Some(parent) => openat(parent, name, flags, Mode::empty()),
        None => rustix::fs::open(name, flags, Mode::empty()),
    }
    .map_err(|_| internal("qualification_cache_plan_directory_untrusted"))?;
    Ok(fs::File::from(descriptor))
}

fn open_file(
    directory: &fs::File,
    name: &str,
    prefix: &str,
) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(internal(&format!("{prefix}_path")));
    }
    openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| internal(&format!("{prefix}_unreadable")))
}

fn check_owned_directory(file: &fs::File, error: &str) -> Result<(), OrchestratorError> {
    let stat = rustix::fs::fstat(file).map_err(|_| internal(error))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != crate::filesystem_owner::effective_uid()
    {
        return Err(internal(error));
    }
    Ok(())
}

#[cfg(test)]
#[path = "anchored_tests.rs"]
mod tests;
