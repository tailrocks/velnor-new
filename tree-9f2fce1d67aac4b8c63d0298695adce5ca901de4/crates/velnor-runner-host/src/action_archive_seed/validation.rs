use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::read::MultiGzDecoder;
use tar::{Archive, EntryType};

use super::ActionArchiveSeedError;
use super::archive_paths::{entry_kind, safe_path, validate_members};

const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;
struct BoundedReader<R> {
    inner: R,
    remaining: u64,
    hit_limit: Arc<AtomicBool>,
}

impl<R> BoundedReader<R> {
    fn new(inner: R, limit: u64, hit_limit: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            remaining: limit,
            hit_limit,
        }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0_u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            self.hit_limit.store(true, Ordering::Relaxed);
            return Err(io::Error::other("expanded archive exceeds limit"));
        }
        let buffer_length = u64::try_from(buffer.len())
            .map_err(|_| io::Error::other("expanded archive length overflow"))?;
        let length = usize::try_from(self.remaining.min(buffer_length))
            .map_err(|_| io::Error::other("expanded archive length overflow"))?;
        let count = self.inner.read(&mut buffer[..length])?;
        let consumed = u64::try_from(count)
            .map_err(|_| io::Error::other("expanded archive length overflow"))?;
        self.remaining = self
            .remaining
            .checked_sub(consumed)
            .ok_or_else(|| io::Error::other("expanded archive length overflow"))?;
        Ok(count)
    }
}

pub(super) fn validate_archive(
    path: &Path,
    compressed_size: u64,
) -> Result<(), ActionArchiveSeedError> {
    validate_archive_bounded(path, compressed_size, MAX_EXPANDED_BYTES)
}

#[cfg(test)]
pub(crate) fn validate_archive_with_limit(
    path: &Path,
    compressed_size: u64,
    expanded_limit: u64,
) -> Result<(), ActionArchiveSeedError> {
    validate_archive_bounded(path, compressed_size, expanded_limit)
}

fn validate_archive_bounded(
    path: &Path,
    compressed_size: u64,
    expanded_limit: u64,
) -> Result<(), ActionArchiveSeedError> {
    if compressed_size == 0 || compressed_size > MAX_ARCHIVE_BYTES {
        return Err(ActionArchiveSeedError::SizeLimit);
    }
    let hit_limit = Arc::new(AtomicBool::new(false));
    let decoder = MultiGzDecoder::new(File::open(path).map_err(|_| ActionArchiveSeedError::Io)?);
    let reader = BoundedReader::new(decoder, expanded_limit, Arc::clone(&hit_limit));
    let mut archive = Archive::new(reader);
    let mut members = Vec::new();
    let mut metadata_bytes = 0_usize;
    let mut expanded = 0_u64;
    {
        let mut entries = archive
            .entries()
            .map_err(|error| archive_error(&hit_limit, error))?
            .raw(true);
        for result in &mut entries {
            let entry = result.map_err(|error| archive_error(&hit_limit, error))?;
            let entry_type = entry.header().entry_type();
            if is_extension(entry_type) {
                return Err(ActionArchiveSeedError::UnsafeEntry);
            }
            let name = safe_path(
                &entry
                    .path()
                    .map_err(|_| ActionArchiveSeedError::UnsafeEntry)?,
            )?;
            metadata_bytes = metadata_bytes
                .checked_add(name.len())
                .ok_or(ActionArchiveSeedError::SizeLimit)?;
            let kind = entry_kind(entry_type, &entry, &mut metadata_bytes)?;
            if members.len() >= MAX_ENTRIES || metadata_bytes > MAX_METADATA_BYTES {
                return Err(ActionArchiveSeedError::SizeLimit);
            }
            expanded = expanded
                .checked_add(entry.size())
                .ok_or(ActionArchiveSeedError::SizeLimit)?;
            if expanded > expanded_limit {
                return Err(ActionArchiveSeedError::SizeLimit);
            }
            members.push((name, kind));
        }
    }
    let mut reader = archive.into_inner();
    drain_archive(&mut reader, expanded, expanded_limit, &hit_limit)?;
    validate_members(members)
}

fn is_extension(entry_type: EntryType) -> bool {
    entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_pax_global_extensions()
}

fn drain_archive<R: Read>(
    reader: &mut BoundedReader<R>,
    expanded: u64,
    expanded_limit: u64,
    hit_limit: &AtomicBool,
) -> Result<(), ActionArchiveSeedError> {
    let mut trailing = 0_u64;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| archive_error(hit_limit, error))?;
        if count == 0 {
            break;
        }
        if buffer[..count].iter().any(|byte| *byte != 0) {
            return Err(ActionArchiveSeedError::InvalidArchive);
        }
        trailing = trailing
            .checked_add(u64::try_from(count).map_err(|_| ActionArchiveSeedError::SizeLimit)?)
            .ok_or(ActionArchiveSeedError::SizeLimit)?;
        if expanded.saturating_add(trailing) > expanded_limit {
            return Err(ActionArchiveSeedError::SizeLimit);
        }
    }
    Ok(())
}

fn archive_error(hit_limit: &AtomicBool, _error: io::Error) -> ActionArchiveSeedError {
    if hit_limit.load(Ordering::Relaxed) {
        ActionArchiveSeedError::SizeLimit
    } else {
        ActionArchiveSeedError::InvalidArchive
    }
}
