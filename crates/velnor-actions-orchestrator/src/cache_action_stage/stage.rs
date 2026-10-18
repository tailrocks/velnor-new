use super::StageError;
use super::context::StageContext;
use super::receipt::StageReceiptClaim;
use super::stage_parent::{
    StageParent, open_directory_path, open_stage_parent, validate_parent, verify_payload_entry,
};
use rustix::fs::{AtFlags, Mode, OFlags, openat, statat, unlinkat};
use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

const MAX_STAGE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const STAGE_MODE: Mode = Mode::from_bits_retain(0o600);

/// Holds the verified stage file and its parent directory descriptors.
pub struct StageReader {
    file: File,
    parent: StageParent,
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
    /// This method does another check of the kept descriptor and its directory entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the stage file no longer matches its claim.
    pub fn revalidate(&mut self) -> Result<(), StageError> {
        let position = self
            .file
            .stream_position()
            .map_err(|_| "cache_stage_read_failed")?;
        verify_file(&mut self.file, &self.parent, &self.claim)?;
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
    parent: StageParent,
    cleanup_on_drop: bool,
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
    /// This method gives an error if the stage file changed or its validation results are unsatisfactory.
    pub fn finish(mut self) -> Result<StageReceiptClaim, StageError> {
        self.file.flush().map_err(|_| "cache_stage_write_failed")?;
        self.file
            .sync_all()
            .map_err(|_| "cache_stage_write_failed")?;
        let claim = claim_for_file(&mut self.file, &self.parent)?;
        self.cleanup_on_drop = false;
        Ok(claim)
    }
}

impl Drop for StageWriter {
    fn drop(&mut self) {
        if !self.cleanup_on_drop {
            return;
        }
        let Ok(file_stat) = rustix::fs::fstat(self.file.as_fd()) else {
            return;
        };
        if validate_stat(&file_stat).is_err() || self.parent.revalidate().is_err() {
            return;
        }
        let Ok(parent) = self.parent.file_directory() else {
            return;
        };
        let Ok(path_stat) = statat(
            parent,
            self.parent.payload_name().as_c_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        ) else {
            return;
        };
        if path_stat.st_dev != file_stat.st_dev
            || path_stat.st_ino != file_stat.st_ino
            || path_stat.st_mode != file_stat.st_mode
            || path_stat.st_uid != file_stat.st_uid
            || path_stat.st_nlink != file_stat.st_nlink
        {
            return;
        }
        match unlinkat(
            parent,
            self.parent.payload_name().as_c_str(),
            AtFlags::empty(),
        ) {
            Ok(()) | Err(_) => {}
        }
    }
}

impl Write for StageWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.parent.revalidate().map_err(io::Error::other)?;
        let position = self.file.stream_position()?;
        let end = position
            .checked_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("cache stage size overflow"))?;
        if end > MAX_STAGE_BYTES {
            return Err(io::Error::other("cache stage size limit exceeded"));
        }
        let written = self.file.write(buffer)?;
        self.parent.revalidate().map_err(io::Error::other)?;
        Ok(written)
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

/// This function makes a new stage file at the specified path. It does not use an existing file.
///
/// # Errors
///
/// Returns an error when the path already exists or cannot be safely opened.
pub fn create_stage(context: &StageContext) -> Result<StageWriter, StageError> {
    let parent = open_stage_parent(context, true)?;
    parent.revalidate()?;
    let descriptor = openat(
        parent.file_directory()?,
        parent.payload_name().as_c_str(),
        OFlags::RDWR
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        STAGE_MODE,
    )
    .map_err(|_| "cache_stage_create_failed")?;
    let file = File::from(descriptor);
    let mut writer = StageWriter {
        file,
        parent,
        cleanup_on_drop: true,
    };
    rustix::fs::fchmod(writer.file.as_fd(), STAGE_MODE).map_err(|_| "cache_stage_create_failed")?;
    let stat = rustix::fs::fstat(writer.file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    validate_stat(&stat)?;
    if stat.st_size != 0 || !path_matches(&writer.parent, &stat)? {
        return Err("cache_stage_changed");
    }
    writer
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_create_failed")?;
    Ok(writer)
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
    let parent = open_stage_parent(context, false)?;
    let descriptor = openat(
        parent.file_directory()?,
        parent.payload_name().as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_open_failed")?;
    let mut file = File::from(descriptor);
    verify_file(&mut file, &parent, claim)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_read_failed")?;
    Ok(StageReader {
        file,
        parent,
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
    let parent = open_stage_parent(context, false)?;
    let descriptor = openat(
        parent.file_directory()?,
        parent.payload_name().as_c_str(),
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_stage_open_failed")?;
    let mut file = File::from(descriptor);
    verify_file(&mut file, &parent, claim)?;
    let mut writer = StageWriter {
        file,
        parent,
        cleanup_on_drop: true,
    };
    writer.parent.revalidate()?;
    writer
        .file
        .set_len(0)
        .map_err(|_| "cache_stage_write_failed")?;
    writer
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_stage_write_failed")?;
    Ok(writer)
}

fn verify_file(
    file: &mut File,
    parent: &StageParent,
    claim: &StageReceiptClaim,
) -> Result<(), StageError> {
    let stat = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    validate_stat(&stat)?;
    if u64::try_from(stat.st_dev).ok() != Some(claim.device())
        || stat.st_ino != claim.inode()
        || u64::try_from(stat.st_size).ok() != Some(claim.length())
        || !path_matches(parent, &stat)?
    {
        return Err("cache_stage_claim_mismatch");
    }
    let digest = digest_file(file)?;
    if &digest != claim.digest() {
        return Err("cache_stage_claim_mismatch");
    }
    let after = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    if !same_version(&stat, &after) || !path_matches(parent, &after)? {
        return Err("cache_stage_changed");
    }
    Ok(())
}

fn claim_for_file(file: &mut File, parent: &StageParent) -> Result<StageReceiptClaim, StageError> {
    let before = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    validate_stat(&before)?;
    if u64::try_from(before.st_size).unwrap_or(u64::MAX) > MAX_STAGE_BYTES
        || !path_matches(parent, &before)?
    {
        return Err("cache_stage_changed");
    }
    let digest = digest_file(file)?;
    let after = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_stage_stat_failed")?;
    if !same_version(&before, &after) || !path_matches(parent, &after)? {
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
        || stat.st_mode & 0o7777 != STAGE_MODE.bits()
        || stat.st_nlink != 1
        || stat.st_size < 0
        || u64::try_from(stat.st_size).unwrap_or(u64::MAX) > MAX_STAGE_BYTES
    {
        return Err("cache_stage_owner_kind_or_mode_invalid");
    }
    Ok(())
}

fn path_matches(parent: &StageParent, file: &rustix::fs::Stat) -> Result<bool, StageError> {
    verify_payload_entry(parent, file)
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
