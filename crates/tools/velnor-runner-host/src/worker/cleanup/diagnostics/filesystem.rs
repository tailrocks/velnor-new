//! Descriptor-relative, owner-checked storage for diagnostic receipts.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{
    AtFlags, Mode, OFlags, fchmod, mkdirat, open, openat, renameat, statat, unlinkat,
};
use rustix::io::Errno;
use rustix::process::geteuid;
use sha2::{Digest, Sha256};

use crate::HostError;

use super::DiagnosticsReceipt;

const PRIVATE_DIRECTORY_MODE: u16 = 0o700;
const PRIVATE_FILE_MODE: u16 = 0o600;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn effective_uid() -> u32 {
    geteuid().as_raw()
}

/// Open every path component relative to a retained descriptor, rejecting symlinks and owners
/// outside the service account/root trust boundary.
pub(super) fn open_trusted_directory(path: &Path, owner: u32) -> Result<File, HostError> {
    if !path.is_absolute() {
        return Err(HostError::Path);
    }
    let root = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| HostError::Path)?;
    let mut directory = File::from(root);
    validate_trusted_ancestor(&directory.metadata().map_err(|_| HostError::Path)?, owner)?;

    for component in path.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(HostError::Path);
        };
        let fd = openat(
            &directory,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?;
        directory = File::from(fd);
        validate_trusted_ancestor(&directory.metadata().map_err(|_| HostError::Path)?, owner)?;
    }
    Ok(directory)
}

pub(super) fn validate_protected_parent(directory: &File, owner: u32) -> Result<(), HostError> {
    let metadata = directory.metadata().map_err(|_| HostError::Path)?;
    let mode = metadata.mode() & 0o7777;
    if !metadata.is_dir()
        || metadata.uid() != owner
        || mode & 0o700 != 0o700
        || mode & 0o022 != 0
        || mode & 0o007 != 0
        || mode & 0o7000 != 0
    {
        return Err(HostError::Path);
    }
    Ok(())
}

pub(super) fn open_private_directory_at(
    parent: &File,
    name: &str,
    owner: u32,
    create: bool,
) -> Result<Option<File>, HostError> {
    validate_name(name)?;
    let created = if create {
        match mkdirat(
            parent,
            name,
            Mode::from_bits_truncate(PRIVATE_DIRECTORY_MODE),
        ) {
            Ok(()) => true,
            Err(Errno::EXIST) => false,
            Err(_) => return Err(HostError::Path),
        }
    } else {
        false
    };
    if created {
        parent.sync_all().map_err(|_| HostError::Path)?;
    }
    let fd = match openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) if !create => return Ok(None),
        Err(_) => return Err(HostError::Path),
    };
    let directory = File::from(fd);
    if created {
        fchmod(&directory, Mode::from_bits_truncate(PRIVATE_DIRECTORY_MODE))
            .map_err(|_| HostError::Path)?;
    }
    validate_private_directory(&directory, owner)?;
    Ok(Some(directory))
}

pub(super) fn validate_private_directory(directory: &File, owner: u32) -> Result<(), HostError> {
    let metadata = directory.metadata().map_err(|_| HostError::Path)?;
    if !metadata.is_dir()
        || metadata.uid() != owner
        || metadata.mode() & 0o7777 != u32::from(PRIVATE_DIRECTORY_MODE)
    {
        return Err(HostError::Path);
    }
    Ok(())
}

pub(super) fn require_absent_at(parent: &File, name: &str) -> Result<(), HostError> {
    validate_name(name)?;
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => Ok(()),
        Ok(_) => Err(HostError::Identity),
        Err(_) => Err(HostError::Path),
    }
}

pub(super) fn read_private_file_at(
    parent: &File,
    name: &str,
    owner: u32,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, HostError> {
    validate_name(name)?;
    let fd = match openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(_) => return Err(HostError::Path),
    };
    let file = File::from(fd);
    let metadata = file.metadata().map_err(|_| HostError::Path)?;
    if !metadata.is_file()
        || metadata.uid() != owner
        || metadata.mode() & 0o7777 != u32::from(PRIVATE_FILE_MODE)
        || metadata.len() > u64::try_from(max_bytes).map_err(|_| HostError::Frame)?
    {
        return Err(HostError::Path);
    }
    let mut bytes =
        Vec::with_capacity(usize::try_from(metadata.len()).map_err(|_| HostError::Frame)?);
    file.take(
        u64::try_from(max_bytes)
            .map_err(|_| HostError::Frame)?
            .saturating_add(1),
    )
    .read_to_end(&mut bytes)
    .map_err(|_| HostError::Path)?;
    if bytes.len() > max_bytes {
        return Err(HostError::Frame);
    }
    Ok(Some(bytes))
}

pub(super) fn atomic_write_at(
    parent: &File,
    name: &str,
    owner: u32,
    bytes: &[u8],
) -> Result<(), HostError> {
    validate_name(name)?;
    let mut temporary = create_temp(parent, name)?;
    temporary
        .file
        .write_all(bytes)
        .map_err(|_| HostError::Path)?;
    temporary.file.sync_all().map_err(|_| HostError::Path)?;
    let metadata = temporary.file.metadata().map_err(|_| HostError::Path)?;
    if !metadata.is_file()
        || metadata.uid() != owner
        || metadata.mode() & 0o7777 != u32::from(PRIVATE_FILE_MODE)
    {
        return Err(HostError::Path);
    }
    renameat(parent, &temporary.name, parent, name).map_err(|_| HostError::Path)?;
    temporary.persisted = true;
    parent.sync_all().map_err(|_| HostError::Path)
}

pub(super) fn sync_directory(directory: &File) -> Result<(), HostError> {
    directory.sync_all().map_err(|_| HostError::Path)
}

pub(super) fn verify_digest(bytes: &[u8], receipt: &DiagnosticsReceipt) -> Result<(), HostError> {
    if u64::try_from(bytes.len()).map_err(|_| HostError::Frame)? != receipt.bytes()
        || digest_hex(bytes) != receipt.sha256()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn digest_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn validate_trusted_ancestor(metadata: &fs::Metadata, owner: u32) -> Result<(), HostError> {
    let mode = metadata.mode() & 0o7777;
    let is_trusted_owner = permitted_owner(metadata.uid(), owner);
    let has_safe_write_policy = mode & 0o022 == 0 || (metadata.uid() == 0 && mode & 0o1000 != 0);
    if !metadata.is_dir() || !is_trusted_owner || !has_safe_write_policy {
        return Err(HostError::Path);
    }
    Ok(())
}

pub(super) fn permitted_owner(directory_uid: u32, service_uid: u32) -> bool {
    directory_uid == service_uid || directory_uid == 0
}

fn validate_name(name: &str) -> Result<(), HostError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(HostError::Path);
    }
    Ok(())
}

struct TempFile {
    parent: File,
    file: File,
    name: String,
    persisted: bool,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.persisted {
            let _cleanup_result = unlinkat(&self.parent, &self.name, AtFlags::empty());
        }
    }
}

fn create_temp(parent: &File, target: &str) -> Result<TempFile, HostError> {
    let parent = parent.try_clone().map_err(|_| HostError::Path)?;
    for _ in 0..16 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".{target}.{}.{}.tmp", std::process::id(), sequence);
        let fd = match openat(
            &parent,
            &name,
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK,
            Mode::from_bits_truncate(PRIVATE_FILE_MODE),
        ) {
            Ok(fd) => fd,
            Err(Errno::EXIST) => continue,
            Err(_) => return Err(HostError::Path),
        };
        let file = File::from(fd);
        fchmod(&file, Mode::from_bits_truncate(PRIVATE_FILE_MODE)).map_err(|_| HostError::Path)?;
        return Ok(TempFile {
            parent,
            file,
            name,
            persisted: false,
        });
    }
    Err(HostError::Path)
}
