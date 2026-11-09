// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::Instant;

use sha2::{Digest, Sha256};
use tar::{Archive, EntryType};

use super::model::{
    ArchiveFiles, Blob, MAX_ARCHIVE_METADATA_BYTES, MAX_METADATA_BYTES, MAX_WORKER_ENTRIES,
};
use super::{ProviderFailure, hex, valid_hex};

pub(super) fn hash_projected(held: &File, deadline: Instant) -> Result<String, ProviderFailure> {
    hash_held_archive_with(held, 4 * 1024 * 1024 * 1024, Some(deadline))
}

fn hash_held_archive_with(
    held: &File,
    limit: u64,
    deadline: Option<Instant>,
) -> Result<String, ProviderFailure> {
    let metadata = held.metadata().map_err(|_| ProviderFailure::Archive)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(ProviderFailure::Archive);
    }
    let mut file = held.try_clone().map_err(|_| ProviderFailure::Archive)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ProviderFailure::Archive)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        check_deadline(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ProviderFailure::Archive)?)
            .ok_or(ProviderFailure::Archive)?;
        if total > limit {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..count]);
    }
    check_deadline(deadline)?;
    if total != metadata.len() {
        return Err(ProviderFailure::Archive);
    }
    Ok(hex(&hasher.finalize()))
}

pub(super) fn read_projected(
    held: &File,
    deadline: Instant,
) -> Result<ArchiveFiles, ProviderFailure> {
    read_archive_with(held, 4 * 1024 * 1024 * 1024, Some(deadline), true)
}

fn read_archive_with(
    held: &File,
    limit: u64,
    deadline: Option<Instant>,
    projected: bool,
) -> Result<ArchiveFiles, ProviderFailure> {
    let metadata = held.metadata().map_err(|_| ProviderFailure::Archive)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(ProviderFailure::Archive);
    }
    let mut file = held.try_clone().map_err(|_| ProviderFailure::Archive)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ProviderFailure::Archive)?;
    if projected {
        scan_projected_framing(&mut file, metadata.len(), deadline)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| ProviderFailure::Archive)?;
    }
    let mut archive = Archive::new(file);
    let entries = archive.entries().map_err(|_| ProviderFailure::Archive)?;
    let mut files = HashMap::new();
    let mut blobs = HashMap::new();
    let mut directories = HashSet::new();
    let mut total = 0_u64;
    let mut entry_count = 0_usize;
    for entry in entries {
        check_deadline(deadline)?;
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        entry_count = entry_count.checked_add(1).ok_or(ProviderFailure::Archive)?;
        if entry_count > if projected { 128 } else { MAX_WORKER_ENTRIES } {
            return Err(ProviderFailure::Archive);
        }
        match read_archive_entry(&mut entry, &mut total, limit, deadline, projected)? {
            ArchiveEntry::Directory(path) => {
                if !["blobs", "blobs/sha256"].contains(&path.as_str()) || !directories.insert(path)
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            ArchiveEntry::FixedFile(path, bytes) => {
                if files.insert(path, bytes).is_some() {
                    return Err(ProviderFailure::Archive);
                }
            }
            ArchiveEntry::Blob(path, blob) => {
                if blobs.insert(path, blob).is_some() {
                    return Err(ProviderFailure::Archive);
                }
            }
        }
    }
    let expected_files: &[&str] = if projected {
        &["oci-layout", "index.json"]
    } else {
        &["oci-layout", "index.json", "manifest.json"]
    };
    if (!projected && directories != HashSet::from(["blobs".to_owned(), "blobs/sha256".to_owned()]))
        || !expected_files.iter().all(|name| files.contains_key(*name))
        || files.len() != expected_files.len()
        || blobs.is_empty()
    {
        return Err(ProviderFailure::Archive);
    }
    check_deadline(deadline)?;
    Ok(ArchiveFiles { files, blobs })
}

enum ArchiveEntry {
    Directory(String),
    FixedFile(String, Vec<u8>),
    Blob(String, Blob),
}

fn read_archive_entry<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    total: &mut u64,
    limit: u64,
    deadline: Option<Instant>,
    projected: bool,
) -> Result<ArchiveEntry, ProviderFailure> {
    let raw_path = entry
        .path()
        .map_err(|_| ProviderFailure::Archive)?
        .to_str()
        .ok_or(ProviderFailure::Archive)?
        .to_owned();
    let kind = entry.header().entry_type();
    let path = if kind == EntryType::Directory {
        raw_path.strip_suffix('/').unwrap_or(&raw_path).to_owned()
    } else {
        if raw_path.ends_with('/') {
            return Err(ProviderFailure::Archive);
        }
        raw_path
    };
    if !safe_path(&path) {
        return Err(ProviderFailure::Archive);
    }
    match kind {
        EntryType::Directory => Ok(ArchiveEntry::Directory(path)),
        EntryType::Regular => {
            let size = entry.size();
            let offset = entry.raw_file_position();
            *total = total.checked_add(size).ok_or(ProviderFailure::Archive)?;
            if *total > limit {
                return Err(ProviderFailure::Archive);
            }
            if is_fixed_file(&path) {
                let limit = match path.as_str() {
                    "oci-layout" => 1024,
                    "index.json" if projected => 1024 * 1024,
                    "index.json" => 64 * 1024,
                    "manifest.json" => 256 * 1024,
                    _ => return Err(ProviderFailure::Archive),
                };
                Ok(ArchiveEntry::FixedFile(
                    path,
                    read_bounded_entry(entry, size, limit)?,
                ))
            } else {
                if projected && size > 1024 * 1024 * 1024 {
                    return Err(ProviderFailure::Archive);
                }
                let digest = path
                    .strip_prefix("blobs/sha256/")
                    .filter(|value| valid_hex(value, 64))
                    .ok_or(ProviderFailure::Archive)?;
                let metadata_limit = (size <= MAX_METADATA_BYTES).then_some(size);
                let (actual, metadata) = hash_entry(entry, size, metadata_limit, deadline)?;
                if actual != digest {
                    return Err(ProviderFailure::Archive);
                }
                Ok(ArchiveEntry::Blob(
                    path,
                    Blob {
                        offset: Some(offset),
                        digest: actual,
                        size,
                        metadata,
                    },
                ))
            }
        }
        _ => Err(ProviderFailure::Archive),
    }
}

pub(super) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

fn is_fixed_file(path: &str) -> bool {
    ["oci-layout", "index.json", "manifest.json"].contains(&path)
}

pub(super) fn read_bounded_entry<R: Read>(
    reader: &mut R,
    size: u64,
    limit: u64,
) -> Result<Vec<u8>, ProviderFailure> {
    if size > limit || size > MAX_ARCHIVE_METADATA_BYTES {
        return Err(ProviderFailure::Archive);
    }
    let capacity = usize::try_from(size).map_err(|_| ProviderFailure::Archive)?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut bounded = reader.take(size);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|_| ProviderFailure::Archive)?;
    if u64::try_from(bytes.len()).map_err(|_| ProviderFailure::Archive)? != size {
        return Err(ProviderFailure::Archive);
    }
    Ok(bytes)
}

fn hash_entry<R: Read>(
    reader: &mut R,
    size: u64,
    metadata_limit: Option<u64>,
    deadline: Option<Instant>,
) -> Result<(String, Option<Vec<u8>>), ProviderFailure> {
    let mut hasher = Sha256::new();
    let mut metadata = metadata_limit
        .map(|limit| usize::try_from(limit).map(Vec::with_capacity))
        .transpose()
        .map_err(|_| ProviderFailure::Archive)?;
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut bounded = reader.take(size);
    loop {
        check_deadline(deadline)?;
        let read = bounded
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| ProviderFailure::Archive)?)
            .ok_or(ProviderFailure::Archive)?;
        if total > size {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..read]);
        if let Some(metadata) = &mut metadata {
            metadata.extend_from_slice(&buffer[..read]);
        }
    }
    if total != size {
        return Err(ProviderFailure::Archive);
    }
    Ok((hex(&hasher.finalize()), metadata))
}

fn check_deadline(deadline: Option<Instant>) -> Result<(), ProviderFailure> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        Err(ProviderFailure::Timeout)
    } else {
        Ok(())
    }
}

fn scan_projected_framing(
    file: &mut File,
    archive_size: u64,
    deadline: Option<Instant>,
) -> Result<(), ProviderFailure> {
    const BLOCK: usize = 512;
    let mut header = [0_u8; BLOCK];
    let mut entries = 0_usize;
    loop {
        check_deadline(deadline)?;
        file.read_exact(&mut header)
            .map_err(|_| ProviderFailure::Archive)?;
        if header.iter().all(|byte| *byte == 0) {
            file.read_exact(&mut header)
                .map_err(|_| ProviderFailure::Archive)?;
            if header.iter().any(|byte| *byte != 0) {
                return Err(ProviderFailure::Archive);
            }
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                check_deadline(deadline)?;
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| ProviderFailure::Archive)?;
                if count == 0 {
                    return Ok(());
                }
                if buffer[..count].iter().any(|byte| *byte != 0) {
                    return Err(ProviderFailure::Archive);
                }
            }
        }
        entries = entries.checked_add(1).ok_or(ProviderFailure::Archive)?;
        if entries > 128 || matches!(header[156], b'x' | b'g' | b'L' | b'K') {
            return Err(ProviderFailure::Archive);
        }
        let size = strict_tar_size(&header[124..136])?;
        let padded = size
            .checked_add((BLOCK - 1) as u64)
            .ok_or(ProviderFailure::Archive)?
            / BLOCK as u64
            * BLOCK as u64;
        let next = file
            .stream_position()
            .map_err(|_| ProviderFailure::Archive)?
            .checked_add(padded)
            .filter(|position| *position <= archive_size)
            .ok_or(ProviderFailure::Archive)?;
        file.seek(SeekFrom::Start(next))
            .map_err(|_| ProviderFailure::Archive)?;
    }
}

pub(super) fn strict_tar_size(field: &[u8]) -> Result<u64, ProviderFailure> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut value = 0_u64;
        for (index, byte) in field.iter().enumerate() {
            let byte = if index == 0 { byte & 0x7f } else { *byte };
            value = value
                .checked_mul(256)
                .and_then(|value| value.checked_add(u64::from(byte)))
                .ok_or(ProviderFailure::Archive)?;
        }
        return Ok(value);
    }
    field
        .iter()
        .copied()
        .take_while(|byte| *byte != 0 && *byte != b' ')
        .try_fold(0_u64, |value, byte| {
            if !(b'0'..=b'7').contains(&byte) {
                return Err(ProviderFailure::Archive);
            }
            value
                .checked_mul(8)
                .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                .ok_or(ProviderFailure::Archive)
        })
}
