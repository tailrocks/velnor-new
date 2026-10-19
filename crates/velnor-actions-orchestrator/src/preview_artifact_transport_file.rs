//! Exclusive, descriptor-held service archives; no archive-sized memory buffer.
use crate::{OrchestratorError, internal::internal};
use rustix::fs::{AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use velnor_actions_mise::CheckDeadline;

pub(crate) struct SealedArchive {
    path: PathBuf,
    parent: File,
    directory: Metadata,
    file: File,
    original: Metadata,
    size: u64,
    digest: String,
}

impl SealedArchive {
    pub(super) fn create(
        parent: &Path,
        id: u64,
        size: u64,
        digest: &str,
    ) -> Result<Self, OrchestratorError> {
        crate::check_evidence::reject_link_components(
            Path::new("/"),
            parent
                .strip_prefix("/")
                .map_err(|_| invalid("path"))?
                .to_str()
                .ok_or_else(|| invalid("path"))?,
        )?;
        let directory = fs::symlink_metadata(parent).map_err(|_| invalid("directory"))?;
        if !directory.is_dir()
            || directory.uid() != crate::filesystem_owner::effective_uid()
            || directory.mode() & 0o022 != 0
        {
            return Err(invalid("directory"));
        }
        let parent_fd = File::from(
            rustix::fs::open(
                parent,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| invalid("directory"))?,
        );
        let held = parent_fd.metadata().map_err(|_| invalid("directory"))?;
        if held.dev() != directory.dev() || held.ino() != directory.ino() {
            return Err(invalid("directory"));
        }
        let name = format!("artifact-{id}.zip");
        let file = File::from(
            rustix::fs::openat(
                &parent_fd,
                name.as_str(),
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| invalid("create"))?,
        );
        let original = file.metadata().map_err(|_| invalid("stat"))?;
        Ok(Self {
            path: parent.join(name),
            parent: parent_fd,
            directory,
            file,
            original,
            size,
            digest: digest.to_owned(),
        })
    }

    pub(super) fn destination_file(&mut self) -> &mut File {
        &mut self.file
    }

    pub(super) fn seal(&mut self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        self.file.sync_all().map_err(|_| invalid("sync"))?;
        let written = self.file.metadata().map_err(|_| invalid("stat"))?;
        if !stable_identity(&self.original, &written) {
            return Err(invalid("identity"));
        }
        self.original = written;
        self.verify(deadline)
    }

    pub(crate) fn reader(&mut self) -> Result<&mut File, OrchestratorError> {
        self.identity()?;
        self.file.rewind().map_err(|_| invalid("seek"))?;
        Ok(&mut self.file)
    }

    pub(crate) fn verify(&mut self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        self.identity()?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| invalid("seek"))?;
        let mut hash = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            super::checkpoint(deadline)?;
            let count = self.file.read(&mut buffer).map_err(|_| invalid("read"))?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .filter(|n| *n <= self.size)
                .ok_or_else(|| invalid("identity"))?;
            hash.update(&buffer[..count]);
        }
        if total != self.size
            || super::super::artifact_zip::hex_digest(hash.finalize()) != self.digest
        {
            return Err(invalid("identity"));
        }
        self.identity()?;
        super::checkpoint(deadline)
    }

    fn identity(&self) -> Result<(), OrchestratorError> {
        let held = self.file.metadata().map_err(|_| invalid("stat"))?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| invalid("stat"))?;
        let directory = self.parent.metadata().map_err(|_| invalid("directory"))?;
        let parent = self.path.parent().ok_or_else(|| invalid("directory"))?;
        let current = fs::symlink_metadata(parent).map_err(|_| invalid("directory"))?;
        if !same(&self.original, &held)
            || !same(&self.original, &named)
            || held.len() != self.size
            || !current.is_dir()
            || current.dev() != self.directory.dev()
            || current.ino() != self.directory.ino()
            || current.mode() != self.directory.mode()
            || current.uid() != self.directory.uid()
            || directory.dev() != current.dev()
            || directory.ino() != current.ino()
        {
            return Err(invalid("identity"));
        }
        Ok(())
    }
}

fn stable_identity(before: &Metadata, now: &Metadata) -> bool {
    now.is_file()
        && now.nlink() == 1
        && now.dev() == before.dev()
        && now.ino() == before.ino()
        && now.uid() == before.uid()
        && now.gid() == before.gid()
        && now.mode() == before.mode()
}

fn same(before: &Metadata, now: &Metadata) -> bool {
    now.is_file()
        && now.nlink() == 1
        && now.dev() == before.dev()
        && now.ino() == before.ino()
        && now.uid() == before.uid()
        && now.gid() == before.gid()
        && now.mode() == before.mode()
        && now.len() == before.len()
        && now.mtime() == before.mtime()
        && now.mtime_nsec() == before.mtime_nsec()
        && now.ctime() == before.ctime()
        && now.ctime_nsec() == before.ctime_nsec()
}

impl Drop for SealedArchive {
    fn drop(&mut self) {
        let Some(name) = self.path.file_name() else {
            return;
        };
        let Ok(held) = rustix::fs::fstat(&self.file) else {
            return;
        };
        if let Ok(named) = rustix::fs::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW)
            && named.st_dev == held.st_dev
            && named.st_ino == held.st_ino
            && named.st_nlink == 1
            && rustix::fs::unlinkat(&self.parent, name, AtFlags::empty()).is_err()
        {
            eprintln!("preview_archive_cleanup_failed");
        }
    }
}

fn invalid(reason: &str) -> OrchestratorError {
    internal(&format!("preview_archive_{reason}"))
}

#[cfg(test)]
#[path = "preview_artifact_transport_file_tests.rs"]
mod tests;
