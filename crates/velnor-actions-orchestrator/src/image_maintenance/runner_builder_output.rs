// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use rustix::fs::{self as rfs, AtFlags, Mode, OFlags, RenameFlags};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(super) struct OutputParent {
    parent: fs::File,
    parent_path: PathBuf,
    target_name: String,
}

pub(super) struct Output {
    parent: fs::File,
    parent_path: PathBuf,
    stage: fs::File,
    stage_path: PathBuf,
    stage_name: String,
    target_name: String,
    created: Vec<(String, fs::File)>,
    expected_files: usize,
}

impl OutputParent {
    pub(super) fn open(target: &Path, root: &Path) -> Result<Self, OrchestratorError> {
        if !target.is_absolute() || target.starts_with(root) {
            return Err(internal("runner_builder_output"));
        }
        let parent_path = target
            .parent()
            .ok_or_else(|| internal("runner_builder_output"))?;
        if parent_path
            .canonicalize()
            .map_err(|_| internal("runner_builder_output"))?
            != parent_path
        {
            return Err(internal("runner_builder_output"));
        }
        let target_name = target
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| internal("runner_builder_output"))?
            .to_owned();
        let parent = fs::File::from(
            rfs::open(
                parent_path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| internal("runner_builder_output"))?,
        );
        let stat = parent
            .metadata()
            .map_err(|_| internal("runner_builder_output"))?;
        if stat.uid() != crate::filesystem_owner::effective_uid() || stat.mode() & 0o077 != 0 {
            return Err(internal("runner_builder_output"));
        }
        match rfs::statat(&parent, target_name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => {}
            _ => return Err(internal("runner_builder_output")),
        }
        Ok(Self {
            parent,
            parent_path: parent_path.to_owned(),
            target_name,
        })
    }
}

impl Output {
    pub(super) fn new(
        destination: OutputParent,
        expected_files: usize,
    ) -> Result<Self, OrchestratorError> {
        let OutputParent {
            parent,
            parent_path,
            target_name,
        } = destination;
        let stage_path = tempfile::Builder::new()
            .prefix(".velnor-runner-author-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&parent_path)
            .map_err(|_| internal("runner_builder_stage"))?
            .keep();
        let stage_name = stage_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| internal("runner_builder_stage"))?
            .to_owned();
        let stage = fs::File::from(
            rfs::openat(
                &parent,
                stage_name.as_str(),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| {
                internal(&format!(
                    "runner_builder_stage_retained:{}",
                    stage_path.display()
                ))
            })?,
        );
        let result = Self {
            parent,
            parent_path,
            stage,
            stage_path,
            stage_name,
            target_name,
            created: Vec::new(),
            expected_files,
        };
        result.check_stage().map_err(|_| {
            internal(&format!(
                "runner_builder_stage_retained:{}",
                result.stage_path.display()
            ))
        })?;
        Ok(result)
    }

    fn check_stage(&self) -> Result<(), OrchestratorError> {
        let held_parent = self
            .parent
            .metadata()
            .map_err(|_| internal("runner_builder_output"))?;
        let named_parent = fs::symlink_metadata(&self.parent_path)
            .map_err(|_| internal("runner_builder_output"))?;
        if !named_parent.is_dir()
            || named_parent.file_type().is_symlink()
            || held_parent.dev() != named_parent.dev()
            || held_parent.ino() != named_parent.ino()
            || named_parent.uid() != crate::filesystem_owner::effective_uid()
            || named_parent.mode() & 0o077 != 0
        {
            return Err(internal("runner_builder_output"));
        }
        let held = rfs::fstat(&self.stage).map_err(|_| internal("runner_builder_stage"))?;
        let named = rfs::statat(
            &self.parent,
            self.stage_name.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| internal("runner_builder_stage"))?;
        if held.st_dev != named.st_dev
            || held.st_ino != named.st_ino
            || held.st_uid != crate::filesystem_owner::effective_uid()
            || held.st_mode & 0o077 != 0
        {
            return Err(internal("runner_builder_stage"));
        }
        Ok(())
    }

    fn same_file(&self, name: &str, file: &fs::File) -> bool {
        match (
            rfs::fstat(file),
            rfs::statat(&self.stage, name, AtFlags::SYMLINK_NOFOLLOW),
        ) {
            (Ok(a), Ok(b)) => {
                a.st_dev == b.st_dev
                    && a.st_ino == b.st_ino
                    && a.st_mode == b.st_mode
                    && a.st_uid == b.st_uid
                    && b.st_nlink == 1
            }
            _ => false,
        }
    }

    pub(super) fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), OrchestratorError> {
        let mut file = self.create(name)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| internal("runner_builder_write"))
    }

    pub(super) fn create(&mut self, name: &str) -> Result<fs::File, OrchestratorError> {
        self.check_stage()?;
        let file = fs::File::from(
            rfs::openat(
                &self.stage,
                name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| internal("runner_builder_create"))?,
        );
        self.created.push((name.to_owned(), file));
        let (_, file) = self
            .created
            .last()
            .ok_or_else(|| internal("runner_builder_create"))?;
        file.try_clone()
            .map_err(|_| internal("runner_builder_create"))
    }

    pub(super) fn publish(mut self) -> Result<(), OrchestratorError> {
        let result = self.publish_inner();
        if result.is_err() {
            self.cleanup()?;
        }
        result?;
        let held =
            rfs::fstat(&self.stage).map_err(|_| internal("runner_builder_output_identity"))?;
        let named = rfs::statat(
            &self.parent,
            self.target_name.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| internal("runner_builder_output_identity"))?;
        if held.st_dev != named.st_dev || held.st_ino != named.st_ino {
            return Err(internal("runner_builder_output_identity"));
        }
        Ok(())
    }

    fn publish_inner(&self) -> Result<(), OrchestratorError> {
        self.check_stage()?;
        if self.created.len() != self.expected_files
            || self
                .created
                .iter()
                .any(|(name, file)| !self.same_file(name, file))
        {
            return Err(internal("runner_builder_stage_identity"));
        }
        let names = fs::read_dir(&self.stage_path)
            .map_err(|_| internal("runner_builder_stage_identity"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| internal("runner_builder_stage_identity"))?;
        if names.len() != self.created.len() {
            return Err(internal("runner_builder_stage_identity"));
        }
        self.check_stage()?;
        rfs::renameat_with(
            &self.parent,
            self.stage_name.as_str(),
            &self.parent,
            self.target_name.as_str(),
            RenameFlags::NOREPLACE,
        )
        .map_err(|_| internal("runner_builder_publish"))?;
        Ok(())
    }

    pub(super) fn cleanup(&mut self) -> Result<(), OrchestratorError> {
        self.check_stage().map_err(|_| {
            internal(&format!(
                "runner_builder_cleanup:{}",
                self.stage_path.display()
            ))
        })?;
        let mut failed = false;
        for (name, file) in &self.created {
            if !self.same_file(name, file)
                || rfs::unlinkat(&self.stage, name.as_str(), AtFlags::empty()).is_err()
            {
                failed = true;
            }
        }
        if !failed
            && rfs::unlinkat(&self.parent, self.stage_name.as_str(), AtFlags::REMOVEDIR).is_err()
        {
            failed = true;
        }
        if failed {
            return Err(internal(&format!(
                "runner_builder_cleanup:{}",
                self.stage_path.display()
            )));
        }
        Ok(())
    }
}
