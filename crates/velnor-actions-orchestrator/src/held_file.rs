use crate::{OrchestratorError, internal::internal};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use velnor_actions_mise::CheckDeadline;

#[derive(Debug)]
pub(crate) struct HeldFile {
    path: PathBuf,
    file: fs::File,
    original: fs::Metadata,
    digest: String,
    size: u64,
}

impl HeldFile {
    pub(crate) fn capture(path: &Path, deadline: CheckDeadline) -> Result<Self, OrchestratorError> {
        reject_links(path)?;
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| invalid("open"))?;
        let file = fs::File::from(fd);
        let original = file.metadata().map_err(|_| invalid("stat"))?;
        if !original.is_file() || original.nlink() != 1 || original.len() > 1024 * 1024 * 1024 {
            return Err(invalid("kind_or_size"));
        }
        let mut held = Self {
            path: path.to_owned(),
            size: original.len(),
            original,
            file,
            digest: String::new(),
        };
        held.verify_identity()?;
        held.digest = held.hash(deadline)?;
        held.verify_identity()?;
        Ok(held)
    }

    pub(crate) fn new(
        path: &Path,
        digest: &str,
        size: u64,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let held = Self::capture(path, deadline)?;
        if held.digest != digest || held.size != size {
            return Err(invalid("digest"));
        }
        Ok(held)
    }

    pub(crate) fn verify(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        self.verify_identity()?;
        if self.hash(deadline)? != self.digest {
            return Err(invalid("digest"));
        }
        self.verify_identity()
    }

    pub(crate) fn read_bytes(
        &self,
        max_size: u64,
        deadline: CheckDeadline,
    ) -> Result<Vec<u8>, OrchestratorError> {
        use std::os::unix::fs::FileExt;

        if self.size > max_size {
            return Err(invalid("read_size"));
        }
        let capacity = usize::try_from(self.size).map_err(|_| invalid("read_size"))?;
        self.verify_identity()?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        let mut hash = Sha256::new();
        while offset < self.size {
            checkpoint(deadline)?;
            let remaining = self.size - offset;
            let length = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| invalid("read_size"))?;
            let count = self
                .file
                .read_at(&mut buffer[..length], offset)
                .map_err(|_| invalid("read"))?;
            if count == 0 {
                return Err(invalid("read_size"));
            }
            bytes.extend_from_slice(&buffer[..count]);
            hash.update(&buffer[..count]);
            offset = offset
                .checked_add(count as u64)
                .filter(|total| *total <= self.size)
                .ok_or_else(|| invalid("read_size"))?;
        }
        checkpoint(deadline)?;
        if offset != self.size || hex(&hash.finalize()) != self.digest {
            return Err(invalid("read_digest"));
        }
        self.verify_identity()?;
        Ok(bytes)
    }

    pub(crate) fn file(&self) -> &fs::File {
        &self.file
    }

    pub(crate) fn verify_identity(&self) -> Result<(), OrchestratorError> {
        reject_links(&self.path)?;
        let held = self.file.metadata().map_err(|_| invalid("stat"))?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| invalid("stat"))?;
        if !held.is_file()
            || !named.is_file()
            || held.nlink() != 1
            || named.nlink() != 1
            || !same_metadata(&self.original, &held)
            || !same_metadata(&self.original, &named)
        {
            return Err(invalid("identity_changed"));
        }
        Ok(())
    }

    fn hash(&self, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
        let mut file = self.file.try_clone().map_err(|_| invalid("clone"))?;
        file.rewind().map_err(|_| invalid("seek"))?;
        let mut hash = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            checkpoint(deadline)?;
            let count = file.read(&mut buffer).map_err(|_| invalid("read"))?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .filter(|n| *n <= self.size)
                .ok_or_else(|| invalid("size"))?;
            hash.update(&buffer[..count]);
        }
        checkpoint(deadline)?;
        if total != self.size {
            return Err(invalid("size"));
        }
        Ok(hex(&hash.finalize()))
    }
}

pub(crate) fn same_metadata(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mode() == b.mode()
        && a.uid() == b.uid()
        && a.gid() == b.gid()
        && a.nlink() == b.nlink()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

pub(crate) fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| invalid("deadline"))
}
fn reject_links(path: &Path) -> Result<(), OrchestratorError> {
    crate::check_evidence::reject_link_components(
        Path::new("/"),
        path.strip_prefix("/")
            .ok()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("path"))?,
    )
}
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
fn invalid(reason: &str) -> OrchestratorError {
    internal(&format!("held_file_{reason}"))
}
