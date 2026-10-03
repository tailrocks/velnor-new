//! Bounded extraction of qualified tool archives.

use std::collections::HashSet;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::GzDecoder;
use lzma_rust2::XzReader;
use tar::EntryType;
use zip::ZipArchive;

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "check_tool_archive_links.rs"]
mod links;
#[path = "check_tool_archive_paths.rs"]
mod paths;

const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENTS: usize = 256;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_GLOBAL_ENTRIES: usize = 200_000;
const XZ_MEMORY_LIMIT_KIB: u32 = 256 * 1024;

#[derive(Clone, Copy)]
enum ArchiveFormat {
    TarGzip,
    TarXz,
    Zip,
}

/// Shared admission budget for every archive in one tool installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchiveBudget {
    /// Remaining decoded regular-file and link-target bytes.
    pub(crate) bytes: u64,
    /// Remaining archive entries.
    pub(crate) entries: usize,
}

impl ArchiveBudget {
    /// Create the bounded installation budget.
    pub(crate) const fn new() -> Self {
        Self {
            bytes: MAX_TOTAL_BYTES,
            entries: MAX_GLOBAL_ENTRIES,
        }
    }

    fn admit_entry(&mut self) -> Result<(), OrchestratorError> {
        if self.entries == 0 {
            return Err(internal("tool_archive_global_entry_limit"));
        }
        self.entries -= 1;
        Ok(())
    }

    fn admit_bytes(&mut self, bytes: u64) -> Result<(), OrchestratorError> {
        if bytes > self.bytes {
            return Err(internal("tool_archive_global_size_limit"));
        }
        self.bytes -= bytes;
        Ok(())
    }
}

impl Default for ArchiveBudget {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract one declared archive into a fresh directory.
pub(crate) fn extract_archive(
    archive: &Path,
    destination: &Path,
    url: &str,
    budget: &mut ArchiveBudget,
) -> Result<(), OrchestratorError> {
    let format = paths::archive_format(url)?;
    let source = open_archive(archive)?;
    paths::create_destination(destination)?;
    let result = match format {
        ArchiveFormat::TarGzip => extract_tar(GzDecoder::new(source), destination, budget),
        ArchiveFormat::TarXz => extract_tar(
            XzReader::new_mem_limit(source, false, XZ_MEMORY_LIMIT_KIB),
            destination,
            budget,
        ),
        ArchiveFormat::Zip => extract_zip(source, destination, budget),
    };
    if let Err(error) = result {
        fs::remove_dir_all(destination).map_err(|cleanup| {
            io_error(
                destination,
                format!("archive failure: {error}; cleanup failure: {cleanup}"),
            )
        })?;
        return Err(error);
    }
    Ok(())
}

fn open_archive(path: &Path) -> Result<File, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path(path, "archive_symlink"));
    }
    if !metadata.is_file() {
        return Err(unsafe_path(path, "archive_not_regular"));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(internal("tool_archive_input_limit"));
    }
    #[cfg(unix)]
    {
        let descriptor = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| io_error(path, error))?;
        let stat = rustix::fs::fstat(&descriptor)
            .map_err(|error| io_error(path, std::io::Error::from(error)))?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
            return Err(unsafe_path(path, "archive_not_regular"));
        }
        if u64::try_from(stat.st_size).unwrap_or(u64::MAX) > MAX_ARCHIVE_BYTES {
            return Err(internal("tool_archive_input_limit"));
        }
        Ok(File::from(descriptor))
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|error| io_error(path, error))
    }
}
fn extract_tar<R: Read>(
    reader: R,
    destination: &Path,
    budget: &mut ArchiveBudget,
) -> Result<(), OrchestratorError> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| archive_error(&error.to_string()))?;
    let mut seen = HashSet::new();
    let mut directories = Vec::new();
    let mut symlinks = Vec::new();
    let mut count = 0_usize;
    let mut total = 0_u64;
    for entry in entries {
        count = count.saturating_add(1);
        if count > MAX_ENTRIES {
            return Err(internal("tool_archive_entry_limit"));
        }
        budget.admit_entry()?;
        let mut entry = entry.map_err(|error| archive_error(&error.to_string()))?;
        let safe = paths::safe_path(entry.path_bytes().as_ref())?;
        if !seen.insert(safe.key.clone()) {
            return Err(internal("tool_archive_duplicate_path"));
        }
        let mode = entry
            .header()
            .mode()
            .map_err(|error| archive_error(&error.to_string()))?;
        match entry.header().entry_type() {
            EntryType::Directory => {
                paths::ensure_directory(destination, &safe.path)?;
                directories.push((safe.path, mode));
            }
            EntryType::Regular | EntryType::Continuous => {
                if safe.trailing_separator {
                    return Err(internal("tool_archive_file_directory_name"));
                }
                let size = entry.size();
                reserve_size(size, &mut total, budget)?;
                paths::write_entry(destination, &safe.path, &mut entry, size, mode)?;
            }
            EntryType::Symlink => {
                let target = entry
                    .link_name_bytes()
                    .ok_or_else(|| internal("tool_archive_link_target"))?;
                let target = links::target_from_bytes(target.as_ref())?;
                let size = entry.size();
                reserve_size(size, &mut total, budget)?;
                links::discard_entry(&mut entry, size)?;
                symlinks.push(links::PendingLink {
                    path: safe.path,
                    target,
                });
            }
            _ => return Err(internal("tool_archive_link_or_special")),
        }
    }
    links::create_links(destination, symlinks)?;
    paths::apply_directory_modes(destination, directories)?;
    Ok(())
}
fn extract_zip(
    mut source: File,
    destination: &Path,
    budget: &mut ArchiveBudget,
) -> Result<(), OrchestratorError> {
    preflight_zip_entries(&mut source)?;
    let mut archive =
        ZipArchive::new(&mut source).map_err(|error| archive_error(&error.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(internal("tool_archive_entry_limit"));
    }
    let mut seen = HashSet::new();
    let mut directories = Vec::new();
    let mut symlinks = Vec::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        budget.admit_entry()?;
        let mut entry = archive
            .by_index(index)
            .map_err(|error| archive_error(&error.to_string()))?;
        let safe = paths::safe_path(entry.name_raw())?;
        if !seen.insert(safe.key.clone()) {
            return Err(internal("tool_archive_duplicate_path"));
        }
        if entry.encrypted() {
            return Err(internal("tool_archive_link_or_encrypted"));
        }
        let mode = entry.unix_mode().unwrap_or(0o644);
        if entry.is_symlink() {
            let size = entry.size();
            reserve_size(size, &mut total, budget)?;
            let target = links::read_target(&mut entry, size)?;
            symlinks.push(links::PendingLink {
                path: safe.path,
                target,
            });
            continue;
        }
        reject_zip_special(mode, entry.is_dir())?;
        if entry.is_dir() {
            paths::ensure_directory(destination, &safe.path)?;
            directories.push((safe.path, mode));
        } else {
            if safe.trailing_separator {
                return Err(internal("tool_archive_file_directory_name"));
            }
            let size = entry.size();
            reserve_size(size, &mut total, budget)?;
            paths::write_entry(destination, &safe.path, &mut entry, size, mode)?;
        }
    }
    links::create_links(destination, symlinks)?;
    paths::apply_directory_modes(destination, directories)?;
    Ok(())
}
fn preflight_zip_entries(source: &mut File) -> Result<(), OrchestratorError> {
    const EOCD_BYTES: u64 = 22 + 65_535 + 20 + 56;
    let length = source
        .metadata()
        .map_err(|error| io_error(Path::new("zip"), error))?
        .len();
    let start = length.saturating_sub(EOCD_BYTES);
    source
        .seek(SeekFrom::Start(start))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    let mut tail = Vec::new();
    source
        .read_to_end(&mut tail)
        .map_err(|error| io_error(Path::new("zip"), error))?;
    let marker = b"PK\x05\x06";
    let eocd = tail
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or_else(|| internal("tool_archive_zip_eocd"))?;
    if tail.len().saturating_sub(eocd) < 22 {
        return Err(internal("tool_archive_zip_eocd"));
    }
    let count = u16::from_le_bytes([tail[eocd + 10], tail[eocd + 11]]);
    let count = if count == u16::MAX {
        zip64_entry_count(&tail, start, eocd)?
    } else {
        u64::from(count)
    };
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(Path::new("zip"), error))?;
    if count > MAX_ENTRIES as u64 {
        return Err(internal("tool_archive_entry_limit"));
    }
    Ok(())
}
fn zip64_entry_count(tail: &[u8], start: u64, eocd: usize) -> Result<u64, OrchestratorError> {
    let marker = b"PK\x06\x07";
    let locator = tail[..eocd]
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or_else(|| internal("tool_archive_zip64_entries"))?;
    if locator.saturating_add(20) > eocd {
        return Err(internal("tool_archive_zip64_entries"));
    }
    let absolute = u64::from_le_bytes(
        tail[locator + 8..locator + 16]
            .try_into()
            .map_err(|_| internal("tool_archive_zip64_entries"))?,
    );
    let relative = absolute
        .checked_sub(start)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| internal("tool_archive_zip64_entries"))?;
    if relative.saturating_add(40) > tail.len() || &tail[relative..relative + 4] != b"PK\x06\x06" {
        return Err(internal("tool_archive_zip64_entries"));
    }
    Ok(u64::from_le_bytes(
        tail[relative + 32..relative + 40]
            .try_into()
            .map_err(|_| internal("tool_archive_zip64_entries"))?,
    ))
}
fn reject_zip_special(mode: u32, directory: bool) -> Result<(), OrchestratorError> {
    let kind = mode & 0o170_000;
    let expected = if directory { 0o040_000 } else { 0o100_000 };
    if kind != 0 && kind != expected {
        return Err(internal("tool_archive_special_entry"));
    }
    Ok(())
}
fn reserve_size(
    size: u64,
    total: &mut u64,
    budget: &mut ArchiveBudget,
) -> Result<(), OrchestratorError> {
    if size > MAX_ENTRY_BYTES {
        return Err(internal("tool_archive_entry_size_limit"));
    }
    let next = total
        .checked_add(size)
        .ok_or_else(|| internal("tool_archive_total_size_limit"))?;
    if next > MAX_TOTAL_BYTES {
        return Err(internal("tool_archive_total_size_limit"));
    }
    budget.admit_bytes(size)?;
    *total = next;
    Ok(())
}
fn archive_error(problem: &str) -> OrchestratorError {
    internal(&format!("tool_archive:{problem}"))
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}

fn unsafe_path_text(path: &str, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.to_owned(),
        reason: reason.to_owned(),
    }
}
#[cfg(test)]
#[path = "check_tool_archive_tests.rs"]
mod tests;
