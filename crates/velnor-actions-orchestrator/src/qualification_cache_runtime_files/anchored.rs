//! Descriptor-relative reads from one pinned qualification run directory.

use std::fs;
use std::io::{Read, Write};
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
            || required_mode.is_some_and(|mode| u64::from(stat.st_mode & 0o7777) != u64::from(mode))
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

    pub(in crate::qualification_cache_runtime) fn write_owned_file(
        &self,
        name: &str,
        bytes: &[u8],
        prefix: &str,
    ) -> Result<(), OrchestratorError> {
        validate_file_name(name, prefix)?;
        let file = openat(
            &self.descriptor,
            name,
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| internal(&format!("{prefix}_create")))?;
        let stat = rustix::fs::fstat(&file).map_err(|_| internal(&format!("{prefix}_write")))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_uid != crate::filesystem_owner::effective_uid()
            || stat.st_mode & 0o7777 != 0o600
        {
            return Err(internal(&format!("{prefix}_untrusted")));
        }
        let mut file = fs::File::from(file);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| internal(&format!("{prefix}_write")))
    }

    pub(in crate::qualification_cache_runtime) fn write_owned_relative_file(
        &self,
        relative: &str,
        bytes: &[u8],
        prefix: &str,
    ) -> Result<(), OrchestratorError> {
        let (parent, name) = self.open_relative_parent(relative, prefix)?;
        let file = openat(
            &parent,
            name.as_str(),
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| internal(&format!("{prefix}_create")))?;
        let stat = rustix::fs::fstat(&file).map_err(|_| internal(&format!("{prefix}_write")))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_uid != crate::filesystem_owner::effective_uid()
            || stat.st_mode & 0o7777 != 0o600
        {
            return Err(internal(&format!("{prefix}_untrusted")));
        }
        let mut file = fs::File::from(file);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| internal(&format!("{prefix}_write")))
    }

    pub(in crate::qualification_cache_runtime) fn read_owned_relative_file(
        &self,
        relative: &str,
        limit: usize,
        required_mode: Option<u16>,
        prefix: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        let (parent, name) = self.open_relative_parent_existing(relative, prefix)?;
        let file = openat(
            &parent,
            name.as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| internal(&format!("{prefix}_unreadable")))?;
        let stat =
            rustix::fs::fstat(&file).map_err(|_| internal(&format!("{prefix}_unreadable")))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_uid != crate::filesystem_owner::effective_uid()
            || required_mode.is_some_and(|mode| u64::from(stat.st_mode & 0o7777) != u64::from(mode))
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

    fn open_relative_parent(
        &self,
        relative: &str,
        prefix: &str,
    ) -> Result<(fs::File, String), OrchestratorError> {
        let (parents, name) = relative_parts(relative, prefix)?;
        let mut current = self
            .descriptor
            .try_clone()
            .map_err(|_| internal(&format!("{prefix}_directory_untrusted")))?;
        for part in parents {
            create_private_directory(&current, &part, prefix)?;
            current = open_directory(Path::new(&part), Some(&current))?;
            check_private_subdirectory(&current, prefix)?;
        }
        Ok((current, name))
    }

    fn open_relative_parent_existing(
        &self,
        relative: &str,
        prefix: &str,
    ) -> Result<(fs::File, String), OrchestratorError> {
        let (parents, name) = relative_parts(relative, prefix)?;
        let mut current = self
            .descriptor
            .try_clone()
            .map_err(|_| internal(&format!("{prefix}_directory_untrusted")))?;
        for part in parents {
            current = open_directory(Path::new(&part), Some(&current))?;
            check_private_subdirectory(&current, prefix)?;
        }
        Ok((current, name))
    }

    pub(in crate::qualification_cache_runtime) fn file_exists(
        &self,
        name: &str,
    ) -> Result<bool, OrchestratorError> {
        match statat(&self.descriptor, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => Ok(true),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(_) => Err(internal("qualification_cache_file_unreadable")),
        }
    }
}

fn relative_parts(
    relative: &str,
    prefix: &str,
) -> Result<(Vec<String>, String), OrchestratorError> {
    if relative.is_empty() || relative.len() > 1_024 || relative.starts_with('/') {
        return Err(internal(&format!("{prefix}_path")));
    }
    let parts = Path::new(relative)
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| internal(&format!("{prefix}_path"))),
            _ => Err(internal(&format!("{prefix}_path"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (name, parents) = parts
        .split_last()
        .ok_or_else(|| internal(&format!("{prefix}_path")))?;
    if parts
        .iter()
        .any(|part| part.is_empty() || part.contains(['/', '\\']))
    {
        return Err(internal(&format!("{prefix}_path")));
    }
    Ok((parents.to_vec(), name.clone()))
}

fn create_private_directory(
    parent: &fs::File,
    name: &str,
    prefix: &str,
) -> Result<(), OrchestratorError> {
    let name = std::ffi::CString::new(name).map_err(|_| internal(&format!("{prefix}_path")))?;
    match rustix::fs::mkdirat(parent, &name, Mode::from_raw_mode(0o700)) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => Ok(()),
        Err(_) => Err(internal(&format!("{prefix}_directory_create"))),
    }
}

fn check_private_subdirectory(file: &fs::File, prefix: &str) -> Result<(), OrchestratorError> {
    let stat =
        rustix::fs::fstat(file).map_err(|_| internal(&format!("{prefix}_directory_untrusted")))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != crate::filesystem_owner::effective_uid()
        || stat.st_mode & 0o7777 != 0o700
    {
        return Err(internal(&format!("{prefix}_directory_untrusted")));
    }
    Ok(())
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
    validate_file_name(name, prefix)?;
    openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| internal(&format!("{prefix}_unreadable")))
}

fn validate_file_name(name: &str, prefix: &str) -> Result<(), OrchestratorError> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        Err(internal(&format!("{prefix}_path")))
    } else {
        Ok(())
    }
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
