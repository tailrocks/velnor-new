//! Deadline-aware ZIP metadata admission, including duplicate/overlap rejection.
use super::{ZipProfile, check, invalid, safe_evidence_name, safe_name};
use crate::OrchestratorError;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
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
    // zip 9 searches at most 65 overlapping 1024-byte windows for the EOCD
    // (22 fixed bytes + the 16-bit comment limit). Reserve 256 operations for
    // those read/seek pairs and fixed EOCD/ZIP64 probes, then 32 per entry for
    // central/local headers, variable fields and consistency checks. Malformed
    // candidate storms still stop at this finite cap, regardless of body size.
    let operations = Rc::new(Cell::new(Some(256 + profile.entries() * 32)));
    let cursor = CheckedCursor {
        cursor: input,
        metadata_ops: Rc::clone(&operations),
    };
    // Native Actions ZIP offsets are absolute within the supplied archive.
    // Detect would scan the entire compressed payload to infer an SFX offset,
    // exhausting metadata fuel on legitimate large files. Known(0) checks the
    // declared directory directly and never falls back to payload scanning.
    let config = zip::read::Config {
        archive_offset: zip::read::ArchiveOffset::Known(0),
    };
    let mut archive =
        zip::ZipArchive::with_config(config, cursor).map_err(|_| invalid("invalid"))?;
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
    let mut evidence_paths = BTreeMap::<String, bool>::new();
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        check()?;
        let file = archive.by_index_raw(index).map_err(|_| invalid("entry"))?;
        let name = file.name().map_err(|_| invalid("path"))?;
        let is_evidence = matches!(profile, ZipProfile::Evidence);
        let is_dir = file.is_dir();
        let mode_type = file.unix_mode().map(|mode| mode & 0o170_000);
        let evidence_type_ok = match (is_dir, mode_type) {
            (true, None | Some(0 | 0o040_000)) => file.size() == 0,
            (false, None | Some(0 | 0o100_000)) => true,
            _ => false,
        };
        if (is_evidence
            && (!safe_evidence_name(&name, is_dir)
                || !evidence_type_ok
                || file.size() > profile.member_unpacked()
                || !seen.insert(name.to_string())
                || evidence_path_collision(&name, is_dir, &mut evidence_paths)))
            || (!is_evidence
                && (!safe_name(&name)
                    || file.is_dir()
                    || file.unix_mode().is_some_and(|mode| {
                        mode & 0o170_000 != 0 && mode & 0o170_000 != 0o100_000
                    })))
            || name.as_bytes() != file.name_raw()
            || file.encrypted()
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

fn evidence_path_collision(name: &str, is_dir: bool, paths: &mut BTreeMap<String, bool>) -> bool {
    let normalized = if is_dir {
        name.trim_end_matches('/')
    } else {
        name
    };
    if paths.contains_key(normalized) {
        return true;
    }
    let components = normalized.split('/').collect::<Vec<_>>();
    for count in 1..components.len() {
        let ancestor = components[..count].join("/");
        if paths.get(&ancestor) == Some(&false) {
            return true;
        }
    }
    if !is_dir
        && paths
            .keys()
            .any(|candidate| candidate.starts_with(&format!("{normalized}/")))
    {
        return true;
    }
    paths.insert(normalized.to_owned(), is_dir);
    false
}
