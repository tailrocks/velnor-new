use super::StageError;
use super::context::StageContext;
use super::receipt::StageReceiptClaim;
use rustix::fs::{AtFlags, Mode, OFlags, open, openat, statat};
use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path};

const MAX_STAGE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const STAGE_MODE: u16 = 0o600;
const ROOT_UID: u32 = 0;

/// Holds the verified stage file and its parent directory descriptors.
pub struct StageReader {
    file: File,
    parent: OwnedFd,
    name: CString,
    claim: StageReceiptClaim,
}

impl fmt::Debug for StageReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StageReader")
            .finish_non_exhaustive()
    }
}

impl StageReader {
    /// Rechecks the retained descriptor and its directory entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the stage file no longer matches its claim.
    pub fn revalidate(&mut self) -> Result<(), StageError> {
        let position = self
            .file
            .stream_position()
            .map_err(|_| "cache_stage_read_failed")?;
        verify_file(&mut self.file, &self.parent, &self.name, &self.claim)?;
        self.file
            .seek(SeekFrom::Start(position))
            .map_err(|_| "cache_stage_read_failed")?;
        Ok(())
    }
}

impl Read for StageReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

impl Seek for StageReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

/// Holds the exclusively created or explicitly claimed stage file.
pub struct StageWriter {
    file: File,
    parent: OwnedFd,
    name: CString,
}

impl fmt::Debug for StageWriter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StageWriter")
            .finish_non_exhaustive()
    }
}

impl StageWriter {
    /// Finishes the write and returns an untrusted compact receipt claim.
    ///
    /// # Errors
    ///
    /// Returns an error when the stage file changed or failed validation.
    pub fn finish(mut self) -> Result<StageReceiptClaim, StageError> {
        self.file.flush().map_err(|_| "cache_stage_write_failed")?;
        self.file
            .sync_all()
            .map_err(|_| "cache_stage_write_failed")?;
        let claim = claim_for_file(&mut self.file, &self.parent, &self.name)?;
        Ok(claim)
    }
}

impl Write for StageWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let position = self.file.stream_position()?;
        let end = position
            .checked_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("cache stage size overflow"))?;
        if end > MAX_STAGE_BYTES {
            return Err(io::Error::other("cache stage size limit exceeded"));
        }
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for StageWriter {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

/// Creates the exact stage file without adopting an existing path.
///
/// # Errors
///
/// Returns an error when the path already exists or cannot be safely opened.
pub fn create_stage(context: &StageContext) -> Result<StageWriter, StageError> {
    let (parent, name) = open_parent(context.action_path())?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDWR
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(STAGE_MODE),
    )
    .map_err(|_| "cache_stage_create_failed")?;
    let mut file = File::from(descriptor);
    rustix::fs::fchmod(file.as_fd(), Mode::from_raw_mode(STAGE_MODE))
        .map_err(|_| "cache_stage_create_failed")?;
    let stat = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    validate_stat(&stat)?;
    if stat.st_size != 0 || !path_matches(&parent, &name, &stat)? {
        return Err("cache_stage_changed");
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_create_failed")?;
    Ok(StageWriter { file, parent, name })
}

/// Opens and verifies a stage file against its untrusted claim.
///
/// # Errors
///
/// Returns an error when the descriptor, path or contents do not match the claim.
pub fn open_stage(
    context: &StageContext,
    claim: &StageReceiptClaim,
) -> Result<StageReader, StageError> {
    let (parent, name) = open_parent(context.action_path())?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_open_failed")?;
    let mut file = File::from(descriptor);
    verify_file(&mut file, &parent, &name, claim)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_read_failed")?;
    Ok(StageReader {
        file,
        parent,
        name,
        claim: claim.clone(),
    })
}

/// Opens a prior claimed stage for an explicit validated rewrite.
///
/// # Errors
///
/// Returns an error when the descriptor, path or contents do not match the claim.
pub fn rewrite_stage(
    context: &StageContext,
    claim: &StageReceiptClaim,
) -> Result<StageWriter, StageError> {
    let (parent, name) = open_parent(context.action_path())?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_open_failed")?;
    let mut file = File::from(descriptor);
    verify_file(&mut file, &parent, &name, claim)?;
    file.set_len(0).map_err(|_| "cache_stage_write_failed")?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_write_failed")?;
    Ok(StageWriter { file, parent, name })
}

fn verify_file(
    file: &mut File,
    parent: &OwnedFd,
    name: &CString,
    claim: &StageReceiptClaim,
) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    validate_stat(&stat)?;
    if u64::try_from(stat.st_dev).ok() != Some(claim.device())
        || stat.st_ino != claim.inode()
        || u64::try_from(stat.st_size).ok() != Some(claim.length())
        || !path_matches(parent, name, &stat)?
    {
        return Err("cache_stage_claim_mismatch");
    }
    let digest = digest_file(file)?;
    if &digest != claim.digest() {
        return Err("cache_stage_claim_mismatch");
    }
    let after = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    if !same_version(&stat, &after) || !path_matches(parent, name, &after)? {
        return Err("cache_stage_changed");
    }
    Ok(())
}

fn claim_for_file(
    file: &mut File,
    parent: &OwnedFd,
    name: &CString,
) -> Result<StageReceiptClaim, StageError> {
    let before = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    validate_stat(&before)?;
    if u64::try_from(before.st_size).unwrap_or(u64::MAX) > MAX_STAGE_BYTES
        || !path_matches(parent, name, &before)?
    {
        return Err("cache_stage_changed");
    }
    let digest = digest_file(file)?;
    let after = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    if !same_version(&before, &after) || !path_matches(parent, name, &after)? {
        return Err("cache_stage_changed");
    }
    Ok(StageReceiptClaim::new(
        u64::try_from(after.st_dev).map_err(|_| "cache_stage_stat_failed")?,
        after.st_ino,
        u64::try_from(after.st_size).map_err(|_| "cache_stage_stat_failed")?,
        digest,
    ))
}

fn digest_file(file: &mut File) -> Result<[u8; 32], StageError> {
    let position = file
        .stream_position()
        .map_err(|_| "cache_stage_read_failed")?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_read_failed")?;
    let mut hasher = blake3::Hasher::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "cache_stage_read_failed")?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_stage_limit")?)
            .ok_or("cache_stage_limit")?;
        if total > MAX_STAGE_BYTES {
            return Err("cache_stage_limit");
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(position))
        .map_err(|_| "cache_stage_read_failed")?;
    Ok(*hasher.finalize().as_bytes())
}

fn validate_stat(stat: &rustix::fs::Stat) -> Result<(), StageError> {
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_uid != crate::process_owner::effective_uid()
        || stat.st_mode & 0o7777 != STAGE_MODE
        || stat.st_nlink != 1
        || stat.st_size < 0
        || u64::try_from(stat.st_size).unwrap_or(u64::MAX) > MAX_STAGE_BYTES
    {
        return Err("cache_stage_owner_kind_or_mode_invalid");
    }
    Ok(())
}

fn path_matches(
    parent: &OwnedFd,
    name: &CString,
    file: &rustix::fs::Stat,
) -> Result<bool, StageError> {
    let path = statat(parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_stage_path_stat_failed")?;
    Ok(path.st_dev == file.st_dev && path.st_ino == file.st_ino && path.st_mode == file.st_mode)
}

fn same_version(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_mode == right.st_mode
        && left.st_uid == right.st_uid
        && left.st_nlink == right.st_nlink
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}

pub(super) fn open_parent(path: &Path) -> Result<(OwnedFd, CString), StageError> {
    let parent = path.parent().ok_or("cache_stage_path_invalid")?;
    let name = path
        .file_name()
        .ok_or("cache_stage_path_invalid")?
        .to_str()
        .ok_or("cache_stage_path_invalid")?;
    let name = CString::new(name).map_err(|_| "cache_stage_path_invalid")?;
    let parent_fd = open_directory_path(parent)?;
    validate_parent(&parent_fd)?;
    Ok((parent_fd, name))
}

fn open_directory_path(path: &Path) -> Result<OwnedFd, StageError> {
    if !path.is_absolute() {
        return Err("cache_stage_path_invalid");
    }
    let mut fd = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_parent_open_failed")?;
    validate_ancestor(&fd)?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(value) => {
                let name = CString::new(value.to_str().ok_or("cache_stage_path_invalid")?)
                    .map_err(|_| "cache_stage_path_invalid")?;
                fd = openat(
                    &fd,
                    name.as_c_str(),
                    OFlags::RDONLY
                        | OFlags::DIRECTORY
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| "cache_stage_parent_open_failed")?;
                validate_ancestor(&fd)?;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("cache_stage_path_invalid");
            }
        }
    }
    Ok(fd)
}

fn validate_ancestor(fd: &OwnedFd) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(fd.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    let owner = stat.st_uid;
    let current_uid = crate::process_owner::effective_uid();
    let mode = stat.st_mode & 0o7777;
    let writable_by_others = mode & 0o0022 != 0;
    let protected_by_sticky_bit = mode & 0o1000 != 0;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory
        || (owner != ROOT_UID && owner != current_uid)
        || (writable_by_others && !protected_by_sticky_bit)
    {
        return Err("cache_stage_ancestor_owner_or_mode_invalid");
    }
    Ok(())
}

fn validate_parent(fd: &OwnedFd) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(fd.as_fd()).map_err(|_| "cache_stage_parent_stat_failed")?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory
        || stat.st_uid != crate::process_owner::effective_uid()
        || stat.st_mode & 0o7022 != 0
    {
        return Err("cache_stage_parent_owner_or_mode_invalid");
    }
    Ok(())
}
