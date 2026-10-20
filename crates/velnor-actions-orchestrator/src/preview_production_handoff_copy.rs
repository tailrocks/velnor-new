//! Stream verified assets between native-owned locations without payload-sized buffers.
use super::{AssetProof, checkpoint, hex};
use crate::filesystem_owner::held_file::HeldFile;
use crate::{OrchestratorError, internal::internal};
use rustix::fs::{AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    os::unix::fs::MetadataExt,
    path::Path,
};
use velnor_actions_mise::CheckDeadline;

pub(crate) fn copy_verified(
    source: &Path,
    destination: &Path,
    proof: &AssetProof,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let source = HeldFile::new(source, &proof.sha256, proof.size, deadline)?;
    let mut input = source.file().try_clone().map_err(|_| invalid("source"))?;
    input.rewind().map_err(|_| invalid("source"))?;
    let mut output = CopyOutput::create(destination)?;
    let mut total = 0_u64;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        checkpoint(deadline)?;
        let count = input.read(&mut buffer).map_err(|_| invalid("source"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|n| *n <= proof.size)
            .ok_or_else(|| invalid("identity"))?;
        output
            .file
            .write_all(&buffer[..count])
            .map_err(|_| invalid("write"))?;
        hash.update(&buffer[..count]);
    }
    if total != proof.size || hex(&hash.finalize()) != proof.sha256 {
        return Err(invalid("identity"));
    }
    output.file.sync_all().map_err(|_| invalid("sync"))?;
    source.verify_identity()?;
    super::recheck(destination, proof, deadline)?;
    output.validate()?;
    checkpoint(deadline)?;
    output.committed = true;
    Ok(())
}

struct CopyOutput {
    parent: File,
    path: std::path::PathBuf,
    directory: std::fs::Metadata,
    file: File,
    committed: bool,
}
impl CopyOutput {
    fn create(path: &Path) -> Result<Self, OrchestratorError> {
        let parent_path = path.parent().ok_or_else(|| invalid("parent"))?;
        let name = path.file_name().ok_or_else(|| invalid("name"))?;
        let parent = File::from(
            rustix::fs::open(
                parent_path,
                OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| invalid("parent"))?,
        );
        let directory = parent.metadata().map_err(|_| invalid("parent"))?;
        if directory.uid() != crate::filesystem_owner::effective_uid()
            || directory.mode() & 0o022 != 0
        {
            return Err(invalid("parent"));
        }
        let file = File::from(
            rustix::fs::openat(
                &parent,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| invalid("create"))?,
        );
        let result = Self {
            parent,
            path: path.to_owned(),
            directory,
            file,
            committed: false,
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), OrchestratorError> {
        let parent = self.path.parent().ok_or_else(|| invalid("parent"))?;
        let named = std::fs::symlink_metadata(parent).map_err(|_| invalid("parent"))?;
        let file = rustix::fs::fstat(&self.file).map_err(|_| invalid("identity"))?;
        let entry = rustix::fs::statat(
            &self.parent,
            self.path.file_name().ok_or_else(|| invalid("name"))?,
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| invalid("identity"))?;
        if !named.is_dir()
            || named.dev() != self.directory.dev()
            || named.ino() != self.directory.ino()
            || named.uid() != self.directory.uid()
            || named.mode() != self.directory.mode()
            || entry.st_dev != file.st_dev
            || entry.st_ino != file.st_ino
            || entry.st_nlink != 1
            || entry.st_mode & 0o170_000 != 0o100_000
        {
            return Err(invalid("identity"));
        }
        Ok(())
    }
}
impl Drop for CopyOutput {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Some(name) = self.path.file_name() else {
            return;
        };
        let Ok(file) = rustix::fs::fstat(&self.file) else {
            return;
        };
        if let Ok(named) = rustix::fs::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW)
            && named.st_dev == file.st_dev
            && named.st_ino == file.st_ino
            && named.st_nlink == 1
            && rustix::fs::unlinkat(&self.parent, name, AtFlags::empty()).is_err()
        {
            eprintln!("preview_copy_cleanup_failed");
        }
    }
}
fn invalid(reason: &str) -> OrchestratorError {
    internal(&format!("preview_copy_{reason}"))
}
