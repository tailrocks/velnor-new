//! Deterministic bounded tar.gz transport for Bun private assets.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::{
    ArchiveBudget, MAX_ARCHIVE_BYTES, MAX_ENTRIES, MAX_ENTRY_BYTES,
};
use crate::internal::internal;

use super::io_error;

pub(super) fn archive_name(id: &str) -> String {
    format!("velnor-pw-assets-{id}.tar.gz")
}

pub(super) fn only_archive(stage: &Path, id: &str) -> Result<PathBuf, OrchestratorError> {
    let expected = archive_name(id);
    let mut entries = fs::read_dir(stage).map_err(|err| io_error(stage, err))?;
    let item = entries
        .next()
        .transpose()
        .map_err(|err| io_error(stage, err))?
        .ok_or_else(|| internal("bun_assets_archive_missing"))?;
    let path = item.path();
    if item.file_name() != std::ffi::OsStr::new(&expected) {
        return Err(internal("bun_assets_archive_unexpected"));
    }
    if entries
        .next()
        .transpose()
        .map_err(|err| io_error(stage, err))?
        .is_some()
    {
        return Err(internal("bun_assets_archive_extra_files"));
    }
    let metadata = fs::symlink_metadata(&path).map_err(|err| io_error(&path, err))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_ARCHIVE_BYTES
    {
        return Err(internal("bun_assets_archive_unsafe"));
    }
    Ok(path)
}

struct BoundedWriter {
    file: File,
    written: u64,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(size) > MAX_ARCHIVE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "archive size limit",
            ));
        }
        let written = self.file.write(bytes)?;
        self.written = self
            .written
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub(super) fn export_archive(deps: &Path, archive: &Path) -> Result<(), OrchestratorError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(archive)
        .map_err(|err| io_error(archive, err))?;
    let encoder = GzEncoder::new(BoundedWriter { file, written: 0 }, Compression::default());
    let mut builder = Builder::new(encoder);
    let mut budget = ArchiveBudget::new();
    let mut count = 0;
    append_directory(&mut builder, deps, deps, &mut budget, &mut count)?;
    builder.finish().map_err(|err| io_error(archive, err))?;
    let encoder = builder.into_inner().map_err(|err| io_error(archive, err))?;
    let bounded = encoder.finish().map_err(|err| io_error(archive, err))?;
    bounded
        .file
        .sync_all()
        .map_err(|err| io_error(archive, err))?;
    set_archive_mode(archive)?;
    Ok(())
}

fn append_directory<W: Write>(
    builder: &mut Builder<W>,
    root: &Path,
    directory: &Path,
    budget: &mut ArchiveBudget,
    count: &mut usize,
) -> Result<(), OrchestratorError> {
    let mut paths = fs::read_dir(directory)
        .map_err(|err| io_error(directory, err))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|err| io_error(directory, err))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| internal("bun_assets_archive_path"))?;
        let archive_path = archive_relative_path(relative)?;
        let metadata = fs::symlink_metadata(&path).map_err(|err| io_error(&path, err))?;
        *count = count.saturating_add(1);
        if *count > MAX_ENTRIES {
            return Err(internal("tool_archive_entry_limit"));
        }
        budget.admit_entry()?;
        if metadata.is_file() {
            if metadata.len() > MAX_ENTRY_BYTES {
                return Err(internal("tool_archive_entry_size_limit"));
            }
            budget.admit_bytes(metadata.len())?;
            let mut header = normalized_header(&metadata, EntryType::Regular, metadata.len());
            builder
                .append_data(
                    &mut header,
                    &archive_path,
                    File::open(&path).map_err(|err| io_error(&path, err))?,
                )
                .map_err(|err| io_error(&path, err))?;
        } else if metadata.is_dir() {
            let mut header = normalized_header(&metadata, EntryType::Directory, 0);
            builder
                .append_data(&mut header, &archive_path, io::empty())
                .map_err(|err| io_error(&path, err))?;
            append_directory(builder, root, &path, budget, count)?;
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|err| io_error(&path, err))?;
            let text = target
                .to_str()
                .ok_or_else(|| internal("tool_archive_non_utf8_link"))?;
            let size = u64::try_from(text.len()).unwrap_or(u64::MAX);
            if size > 4096 {
                return Err(internal("tool_archive_link_size_limit"));
            }
            validate_link_target(root, &path, &target)?;
            budget.admit_bytes(size)?;
            builder
                .append_link(
                    &mut normalized_header(&metadata, EntryType::Symlink, 0),
                    &archive_path,
                    &target,
                )
                .map_err(|err| io_error(&path, err))?;
        } else {
            return Err(internal("tool_archive_special_file"));
        }
    }
    Ok(())
}

fn normalized_header(metadata: &fs::Metadata, kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(file_mode(metadata));
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    let _ = metadata;
    0o644
}

fn archive_relative_path(path: &Path) -> Result<PathBuf, OrchestratorError> {
    if path.as_os_str().is_empty()
        || path.to_str().is_none()
        || path.to_string_lossy().contains('\\')
        || path.as_os_str().to_string_lossy().as_bytes().get(1) == Some(&b':')
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.components().count() > 256
        || path.to_string_lossy().len() > 4096
    {
        return Err(internal("tool_archive_path_component"));
    }
    Ok(path.to_path_buf())
}

fn validate_link_target(root: &Path, path: &Path, target: &Path) -> Result<(), OrchestratorError> {
    if target.is_absolute() || target.to_str().is_none() {
        return Err(internal("tool_archive_link_target"));
    }
    let target_text = target.to_string_lossy();
    if target_text.is_empty()
        || target_text.len() > 4096
        || target_text.contains(['\\', '\0'])
        || target_text.as_bytes().get(1) == Some(&b':')
        || target_text
            .split('/')
            .any(|part| part.is_empty() || part == ".")
    {
        return Err(internal("tool_archive_link_target"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| internal("tool_archive_link_target"))?;
    let resolved = fs::canonicalize(parent.join(target))
        .map_err(|_| internal("tool_archive_link_dangling_or_nonregular"))?;
    let root = fs::canonicalize(root).map_err(|err| io_error(root, err))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(internal("tool_archive_link_escape"));
    }
    Ok(())
}

fn set_archive_mode(path: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|err| io_error(path, err))?;
    }
    Ok(())
}
