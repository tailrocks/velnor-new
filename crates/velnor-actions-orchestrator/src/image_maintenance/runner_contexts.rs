// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, FileExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use velnor_actions_contract::RunnerImageNamedContext;
use velnor_actions_mise::CheckDeadline;

pub(super) const FILES: [(&str, &str); 4] = [
    ("resource_probe", "resource-probe"),
    ("storage_init", "storage-init"),
    ("job_start_hook", "job-start-hook"),
    ("job_started_hook", "velnor-job-start-hook.sh"),
];

type StagedContext = (String, File, Option<(File, RunnerImageNamedContext)>);

pub(super) struct ContextStage {
    path: PathBuf,
    directory: File,
    children: Vec<StagedContext>,
}

impl ContextStage {
    pub(super) fn capture(
        root: &Path,
        input: &Path,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        check_directory(input, true)?;
        let actual = fs::read_dir(input)
            .map_err(|_| internal("runner_context_inventory"))?
            .map(|entry| entry.map(|value| value.file_name()))
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .map_err(|_| internal("runner_context_inventory"))?;
        let expected = FILES
            .into_iter()
            .map(|(name, _)| std::ffi::OsString::from(name))
            .collect();
        if actual != expected {
            return Err(internal("runner_context_inventory"));
        }
        let target = root.join("target");
        if !target.exists() {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&target)
                .map_err(|_| internal("runner_context_target"))?;
        }
        check_directory(&target, false)?;
        let path = target.join("velnor-runner-family");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| internal("runner_context_exclusive"))?;
        let directory = open_directory(&path)?;
        let mut stage = Self {
            path,
            directory,
            children: Vec::new(),
        };
        let result = (|| {
            for (name, filename) in FILES {
                stage.capture_one(input, name, filename, deadline)?;
            }
            stage.revalidate(deadline)
        })();
        if let Err(error) = result {
            stage.close()?;
            return Err(error);
        }
        Ok(stage)
    }

    fn capture_one(
        &mut self,
        input: &Path,
        name: &str,
        filename: &str,
        deadline: CheckDeadline,
    ) -> Result<(), OrchestratorError> {
        let path = self.path.join(name);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| internal("runner_context_directory"))?;
        self.children
            .push((name.to_owned(), open_directory(&path)?, None));
        let source = open_regular(&input.join(name))?;
        let before = source
            .metadata()
            .map_err(|_| internal("runner_context_source"))?;
        if before.len() == 0 || before.len() > 1024 * 1024 * 1024 {
            return Err(internal("runner_context_limit"));
        }
        let entry = self
            .children
            .last_mut()
            .ok_or_else(|| internal("runner_context_create"))?;
        let file = File::from(
            rustix::fs::openat(
                &entry.1,
                filename,
                rustix::fs::OFlags::RDWR
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::EXCL
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .map_err(|_| internal("runner_context_create"))?,
        );
        entry.2 = Some((
            file,
            RunnerImageNamedContext {
                path: format!("target/velnor-runner-family/{name}"),
                sha256: String::new(),
                size_bytes: before.len(),
            },
        ));
        let (file, record) = entry
            .2
            .as_mut()
            .ok_or_else(|| internal("runner_context_create"))?;
        copy(&source, file, before.len(), deadline)?;
        file.set_permissions(fs::Permissions::from_mode(0o555))
            .map_err(|_| internal("runner_context_permissions"))?;
        let after = source
            .metadata()
            .map_err(|_| internal("runner_context_source"))?;
        if identity(&before) != identity(&after) {
            return Err(internal("runner_context_source_changed"));
        }
        record.sha256 = digest(file, before.len(), deadline)?;
        Ok(())
    }

    pub(super) fn records(
        &self,
        role: &str,
    ) -> Result<std::collections::BTreeMap<String, RunnerImageNamedContext>, OrchestratorError>
    {
        let mut records = std::collections::BTreeMap::new();
        for (name, _, entry) in &self.children {
            if role != "runner" && name != "resource_probe" {
                continue;
            }
            let (_, record) = entry
                .as_ref()
                .ok_or_else(|| internal("runner_context_incomplete"))?;
            records.insert(name.clone(), record.clone());
        }
        Ok(records)
    }

    pub(super) fn revalidate(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        same_directory(&self.path, &self.directory)?;
        if fs::read_dir(&self.path)
            .map_err(|_| internal("runner_context_inventory"))?
            .count()
            != self.children.len()
        {
            return Err(internal("runner_context_inventory"));
        }
        for (name, directory, entry) in &self.children {
            let path = self.path.join(name);
            same_directory(&path, directory)?;
            if fs::read_dir(&path)
                .map_err(|_| internal("runner_context_inventory"))?
                .count()
                != 1
            {
                return Err(internal("runner_context_inventory"));
            }
            let (file, record) = entry
                .as_ref()
                .ok_or_else(|| internal("runner_context_incomplete"))?;
            let filename = FILES
                .iter()
                .find(|(key, _)| *key == name)
                .ok_or_else(|| internal("runner_context_name"))?
                .1;
            let named = fs::symlink_metadata(path.join(filename))
                .map_err(|_| internal("runner_context_identity"))?;
            let held = file
                .metadata()
                .map_err(|_| internal("runner_context_identity"))?;
            if identity(&named) != identity(&held)
                || held.nlink() != 1
                || held.mode() & 0o777 != 0o555
                || digest(file, record.size_bytes, deadline)? != record.sha256
            {
                return Err(internal("runner_context_changed"));
            }
        }
        Ok(())
    }

    pub(super) fn file(&self, name: &str) -> Result<&File, OrchestratorError> {
        self.children
            .iter()
            .find(|(key, _, _)| key == name)
            .and_then(|(_, _, entry)| entry.as_ref())
            .map(|(file, _)| file)
            .ok_or_else(|| internal("runner_context_missing"))
    }

    pub(super) fn close(self) -> Result<(), OrchestratorError> {
        same_directory(&self.path, &self.directory)?;
        for (name, directory, entry) in &self.children {
            same_directory(&self.path.join(name), directory)?;
            if let Some((file, _)) = entry {
                let filename = FILES
                    .iter()
                    .find(|(key, _)| *key == name)
                    .ok_or_else(|| internal("runner_context_name"))?
                    .1;
                let named = fs::symlink_metadata(self.path.join(name).join(filename))
                    .map_err(|_| internal("runner_context_cleanup"))?;
                let held = file
                    .metadata()
                    .map_err(|_| internal("runner_context_cleanup"))?;
                if identity(&named) != identity(&held) {
                    return Err(internal("runner_context_cleanup"));
                }
                rustix::fs::unlinkat(directory, filename, rustix::fs::AtFlags::empty())
                    .map_err(|_| internal("runner_context_cleanup"))?;
            }
            rustix::fs::unlinkat(
                &self.directory,
                name.as_str(),
                rustix::fs::AtFlags::REMOVEDIR,
            )
            .map_err(|_| internal("runner_context_cleanup"))?;
        }
        fs::remove_dir(&self.path).map_err(|_| internal("runner_context_cleanup"))
    }
}

fn check_directory(path: &Path, private: bool) -> Result<(), OrchestratorError> {
    let meta = fs::symlink_metadata(path).map_err(|_| internal("runner_context_parent"))?;
    let mask = if private { 0o077 } else { 0o022 };
    if !path.is_absolute()
        || path.canonicalize().ok().as_deref() != Some(path)
        || !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != crate::filesystem_owner::effective_uid()
        || meta.mode() & mask != 0
    {
        return Err(internal("runner_context_parent"));
    }
    Ok(())
}

fn open_directory(path: &Path) -> Result<File, OrchestratorError> {
    rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| internal("runner_context_directory"))
}

fn open_regular(path: &Path) -> Result<File, OrchestratorError> {
    let file = File::from(
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| internal("runner_context_open"))?,
    );
    let meta = file
        .metadata()
        .map_err(|_| internal("runner_context_file"))?;
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(internal("runner_context_file"));
    }
    Ok(file)
}

fn identity(meta: &fs::Metadata) -> (u64, u64, u64, u32, i64, i64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mode(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}

fn same_directory(path: &Path, held: &File) -> Result<(), OrchestratorError> {
    let named =
        fs::symlink_metadata(path).map_err(|_| internal("runner_context_directory_changed"))?;
    let meta = held
        .metadata()
        .map_err(|_| internal("runner_context_directory_changed"))?;
    if !named.is_dir()
        || named.file_type().is_symlink()
        || named.dev() != meta.dev()
        || named.ino() != meta.ino()
        || named.mode() & 0o077 != 0
    {
        return Err(internal("runner_context_directory_changed"));
    }
    Ok(())
}

fn copy(
    source: &File,
    destination: &File,
    size: u64,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut input = source;
    let mut output = destination;
    let mut buffer = vec![0; 64 * 1024];
    let mut copied = 0u64;
    loop {
        deadline
            .remaining()
            .map_err(|_| internal("runner_context_deadline"))?;
        let count = input
            .read(&mut buffer)
            .map_err(|_| internal("runner_context_read"))?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .filter(|n| *n <= size)
            .ok_or_else(|| internal("runner_context_source_changed"))?;
        output
            .write_all(&buffer[..count])
            .map_err(|_| internal("runner_context_write"))?;
    }
    if copied != size {
        return Err(internal("runner_context_source_changed"));
    }
    output
        .sync_all()
        .map_err(|_| internal("runner_context_write"))
}

fn digest(file: &File, size: u64, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    let before = file
        .metadata()
        .map_err(|_| internal("runner_context_stat"))?;
    let mut sha = Sha256::new();
    let mut offset = 0;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        deadline
            .remaining()
            .map_err(|_| internal("runner_context_deadline"))?;
        let count = file
            .read_at(&mut buffer, offset)
            .map_err(|_| internal("runner_context_read"))?;
        if count == 0 {
            break;
        }
        offset = offset
            .checked_add(count as u64)
            .filter(|n| *n <= size)
            .ok_or_else(|| internal("runner_context_changed"))?;
        sha.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| internal("runner_context_stat"))?;
    if offset != size || identity(&before) != identity(&after) {
        return Err(internal("runner_context_changed"));
    }
    Ok(super::source::hex(&sha.finalize()))
}
