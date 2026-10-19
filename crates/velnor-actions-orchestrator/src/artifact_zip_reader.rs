//! Deadline-aware ZIP metadata admission, including duplicate/overlap rejection.
use super::{ZipProfile, check, invalid, safe_name};
use crate::OrchestratorError;
use std::{
    cell::Cell,
    io::{Read, Seek, SeekFrom},
    rc::Rc,
};

pub(super) type Archive<R> = zip::ZipArchive<CheckedCursor<R>>;
pub(super) struct CheckedCursor<R> {
    cursor: R,
    // Prevent malformed central-directory candidates from growing metadata without
    // bound before ZipArchive exposes its decoded entry count. Disabled for bodies.
    metadata_ops: Rc<Cell<Option<usize>>>,
}
impl<R> CheckedCursor<R> {
    fn checkpoint(&self) -> std::io::Result<()> {
        check().map_err(std::io::Error::other)?;
        if let Some(left) = self.metadata_ops.get() {
            self.metadata_ops
                .set(Some(left.checked_sub(1).ok_or_else(|| {
                    std::io::Error::other("ZIP metadata work limit")
                })?));
        }
        Ok(())
    }
}
impl<R: Read> Read for CheckedCursor<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.checkpoint()?;
        self.cursor.read(buffer)
    }
}
impl<R: Seek> Seek for CheckedCursor<R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.checkpoint()?;
        self.cursor.seek(position)
    }
}

pub(super) fn open<R: Read + Seek>(
    mut input: R,
    profile: ZipProfile,
) -> Result<Archive<R>, OrchestratorError> {
    check()?;
    if input.seek(SeekFrom::End(0)).map_err(|_| invalid("seek"))? > profile.compressed() {
        return Err(invalid("compressed_limit"));
    }
    input.rewind().map_err(|_| invalid("seek"))?;
    let operations = Rc::new(Cell::new(Some(profile.entries() * 32)));
    let cursor = CheckedCursor {
        cursor: input,
        metadata_ops: Rc::clone(&operations),
    };
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| invalid("invalid"))?;
    if archive.is_empty() || archive.len() > profile.entries() {
        return Err(invalid("entry_limit"));
    }
    // zip 9 stores names in an IndexMap: the default by_index/by_name path can
    // conceal duplicates. This explicit pass also rejects local/central aliases
    // and overlapping compressed entry ranges before any manifest is selected.
    archive
        .validate_header_consistency()
        .map_err(|_| invalid("header_consistency"))?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        check()?;
        let file = archive.by_index_raw(index).map_err(|_| invalid("entry"))?;
        let name = file.name().map_err(|_| invalid("path"))?;
        if !safe_name(&name)
            || name.as_bytes() != file.name_raw()
            || file.is_dir()
            || file.encrypted()
            || file
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 != 0 && mode & 0o170_000 != 0o100_000)
        {
            return Err(invalid("path_or_type"));
        }
        total = total
            .checked_add(file.size())
            .filter(|n| *n <= profile.unpacked())
            .ok_or_else(|| invalid("unpacked_limit"))?;
    }
    operations.set(None);
    check()?;
    Ok(archive)
}
