use super::{check, invalid, safe_name};
use crate::OrchestratorError;
use rustix::fs::{self as rfs, AtFlags, Mode, OFlags};
use std::{
    collections::BTreeSet,
    fs::{self, File, Metadata},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) struct Output {
    path: PathBuf,
    directory: File,
    identity: Metadata,
    created: Vec<(String, File)>,
    committed: bool,
}
impl Output {
    pub(super) fn open(path: &Path) -> Result<Self, OrchestratorError> {
        let identity = fs::symlink_metadata(path).map_err(|_| invalid("destination"))?;
        if !identity.is_dir()
            || identity.file_type().is_symlink()
            || identity.uid() != crate::filesystem_owner::effective_uid()
            || identity.mode() & 0o022 != 0
        {
            return Err(invalid("destination"));
        }
        let directory = File::from(
            rfs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| invalid("destination"))?,
        );
        let output = Self {
            path: path.into(),
            directory,
            identity,
            created: vec![],
            committed: false,
        };
        output.root_identity()?;
        if fs::read_dir(path)
            .map_err(|_| invalid("destination"))?
            .next()
            .is_some()
        {
            return Err(invalid("destination_not_empty"));
        }
        output.root_identity()?;
        Ok(output)
    }
    fn root_identity(&self) -> Result<(), OrchestratorError> {
        let current =
            fs::symlink_metadata(&self.path).map_err(|_| invalid("destination_identity"))?;
        let held = self
            .directory
            .metadata()
            .map_err(|_| invalid("destination_identity"))?;
        if !current.is_dir()
            || current.file_type().is_symlink()
            || current.dev() != self.identity.dev()
            || current.ino() != self.identity.ino()
            || held.dev() != self.identity.dev()
            || held.ino() != self.identity.ino()
            || current.uid() != self.identity.uid()
            || current.gid() != self.identity.gid()
            || current.mode() != self.identity.mode()
        {
            return Err(invalid("destination_identity"));
        }
        Ok(())
    }
    pub(super) fn create(&mut self, name: &str) -> Result<File, OrchestratorError> {
        check()?;
        if !safe_name(name) {
            return Err(invalid("path"));
        }
        self.root_identity()?;
        let file = File::from(
            rfs::openat(
                &self.directory,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| invalid("output_create"))?,
        );
        self.created.push((name.into(), file));
        self.created
            .last()
            .ok_or_else(|| invalid("output_handle"))?
            .1
            .try_clone()
            .map_err(|_| invalid("output_handle"))
    }
    fn same_file(&self, name: &str, file: &File) -> Result<bool, OrchestratorError> {
        let owned = rfs::fstat(file).map_err(|_| invalid("output_identity"))?;
        match rfs::statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(current) => Ok(current.st_dev == owned.st_dev
                && current.st_ino == owned.st_ino
                && current.st_mode == owned.st_mode
                && current.st_uid == owned.st_uid
                && current.st_gid == owned.st_gid
                && current.st_nlink == 1),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(_) => Err(invalid("output_identity")),
        }
    }
    pub(super) fn validate(&self) -> Result<(), OrchestratorError> {
        self.root_identity()?;
        let names = fs::read_dir(&self.path)
            .map_err(|_| invalid("output_inventory"))?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(|_| invalid("output_inventory"))?;
        if names
            != self
                .created
                .iter()
                .map(|(name, _)| std::ffi::OsString::from(name))
                .collect()
        {
            return Err(invalid("output_inventory"));
        }
        for (name, file) in &self.created {
            check()?;
            if !self.same_file(name, file)? {
                return Err(invalid("output_identity"));
            }
        }
        self.root_identity()
    }
    pub(super) fn commit(&mut self) {
        self.committed = true;
    }
    pub(super) fn rollback(&mut self) -> Result<(), OrchestratorError> {
        let mut failed = false;
        for (name, file) in &self.created {
            match self.same_file(name, file) {
                Ok(true) => {
                    if rfs::unlinkat(&self.directory, name, AtFlags::empty()).is_err() {
                        failed = true;
                    }
                }
                Ok(false) | Err(_) => {
                    failed = true;
                }
            }
        }
        self.created.clear();
        self.committed = true;
        if failed {
            Err(invalid("cleanup_identity"))
        } else {
            Ok(())
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if !self.committed && self.rollback().is_err() {
            eprintln!("artifact_zip_cleanup_failed");
        }
    }
}
