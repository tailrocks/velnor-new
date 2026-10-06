//! Bounded extraction of qualified tool archives.

use std::collections::HashSet;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use tar::EntryType;
use zip::ZipArchive;

use crate::OrchestratorError;
use crate::internal::internal;
use velnor_actions_mise::CheckDeadline;

mod archive_deadline;
mod links;
mod paths;
mod tar_preflight;
mod zip_checks;
use archive_deadline::{DeadlineIo, archive_error, check_deadline, gzip_reader, xz_reader};

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
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let format = paths::archive_format(url)?;
    let extension_entries = match format {
        ArchiveFormat::TarGzip => {
            tar_preflight::preflight_tar(gzip_reader(open_archive(archive)?, deadline))?
        }
        ArchiveFormat::TarXz => {
            tar_preflight::preflight_tar(xz_reader(open_archive(archive)?, deadline))?
        }
        ArchiveFormat::Zip => {
            let mut source = open_archive(archive)?;
            zip_checks::preflight_zip_entries(&mut source, deadline)?;
            0
        }
    };
    for _ in 0..extension_entries {
        budget.admit_entry()?;
    }
    check_deadline(deadline)?;
    let source = open_archive(archive)?;
    paths::create_destination(destination)?;
    let result = match format {
        ArchiveFormat::TarGzip => {
            extract_tar(gzip_reader(source, deadline), destination, budget, deadline)
        }
        ArchiveFormat::TarXz => {
            extract_tar(xz_reader(source, deadline), destination, budget, deadline)
        }
        ArchiveFormat::Zip => extract_zip(source, destination, budget, deadline),
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
    check_deadline(deadline)?;
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
    deadline: CheckDeadline,
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
        check_deadline(deadline)?;
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
                paths::write_entry(destination, &safe.path, &mut entry, size, mode, deadline)?;
            }
            EntryType::Symlink => {
                let target = entry
                    .link_name_bytes()
                    .ok_or_else(|| internal("tool_archive_link_target"))?;
                let target = links::target_from_bytes(target.as_ref())?;
                let size = entry.size();
                reserve_size(size, &mut total, budget)?;
                links::discard_entry(&mut entry, size, deadline)?;
                symlinks.push(links::PendingLink {
                    path: safe.path,
                    target,
                });
            }
            _ => return Err(internal("tool_archive_link_or_special")),
        }
    }
    links::create_links(destination, symlinks, deadline)?;
    paths::apply_directory_modes(destination, directories, deadline)?;
    Ok(())
}

fn extract_zip(
    source: File,
    destination: &Path,
    budget: &mut ArchiveBudget,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let mut source = DeadlineIo::new(source, deadline);
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
        check_deadline(deadline)?;
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
            let target = links::read_target(&mut entry, size, deadline)?;
            symlinks.push(links::PendingLink {
                path: safe.path,
                target,
            });
            continue;
        }
        zip_checks::reject_zip_special(mode, entry.is_dir())?;
        if entry.is_dir() {
            paths::ensure_directory(destination, &safe.path)?;
            directories.push((safe.path, mode));
        } else {
            if safe.trailing_separator {
                return Err(internal("tool_archive_file_directory_name"));
            }
            let size = entry.size();
            reserve_size(size, &mut total, budget)?;
            paths::write_entry(destination, &safe.path, &mut entry, size, mode, deadline)?;
        }
    }
    links::create_links(destination, symlinks, deadline)?;
    paths::apply_directory_modes(destination, directories, deadline)?;
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
mod tests;
