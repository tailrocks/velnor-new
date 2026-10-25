use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use tar::{Archive, EntryType};

use super::model::{
    ArchiveFiles, Blob, MAX_ARCHIVE_METADATA_BYTES, MAX_METADATA_BYTES, MAX_WORKER_ARCHIVE_BYTES,
    MAX_WORKER_ENTRIES,
};
use super::{ProviderFailure, hex, valid_hex};

pub(super) fn hash_archive(path: &Path) -> Result<String, ProviderFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProviderFailure::Archive)?;
    if !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_WORKER_ARCHIVE_BYTES
    {
        return Err(ProviderFailure::Archive);
    }
    let mut file = File::open(path).map_err(|_| ProviderFailure::Archive)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ProviderFailure::Archive)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ProviderFailure::Archive)?)
            .ok_or(ProviderFailure::Archive)?;
        if total > MAX_WORKER_ARCHIVE_BYTES {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..count]);
    }
    if total != metadata.len() {
        return Err(ProviderFailure::Archive);
    }
    Ok(hex(&hasher.finalize()))
}

pub(super) fn read_archive(path: &Path) -> Result<ArchiveFiles, ProviderFailure> {
    let file = File::open(path).map_err(|_| ProviderFailure::Archive)?;
    let mut archive = Archive::new(file);
    let entries = archive.entries().map_err(|_| ProviderFailure::Archive)?;
    let mut files = HashMap::new();
    let mut blobs = HashMap::new();
    let mut directories = HashSet::new();
    let mut total = 0_u64;
    let mut entry_count = 0_usize;
    for entry in entries {
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        entry_count = entry_count.checked_add(1).ok_or(ProviderFailure::Archive)?;
        if entry_count > MAX_WORKER_ENTRIES {
            return Err(ProviderFailure::Archive);
        }
        match read_archive_entry(&mut entry, &mut total)? {
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
    if directories != HashSet::from(["blobs".to_owned(), "blobs/sha256".to_owned()])
        || !["oci-layout", "index.json", "manifest.json"]
            .iter()
            .all(|name| files.contains_key(*name))
        || files.len() != 3
        || blobs.is_empty()
    {
        return Err(ProviderFailure::Archive);
    }
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
            *total = total.checked_add(size).ok_or(ProviderFailure::Archive)?;
            if *total > MAX_WORKER_ARCHIVE_BYTES {
                return Err(ProviderFailure::Archive);
            }
            if is_fixed_file(&path) {
                let limit = match path.as_str() {
                    "oci-layout" => 1024,
                    "index.json" => 64 * 1024,
                    "manifest.json" => 256 * 1024,
                    _ => return Err(ProviderFailure::Archive),
                };
                Ok(ArchiveEntry::FixedFile(
                    path,
                    read_bounded_entry(entry, size, limit)?,
                ))
            } else {
                let digest = path
                    .strip_prefix("blobs/sha256/")
                    .filter(|value| valid_hex(value, 64))
                    .ok_or(ProviderFailure::Archive)?;
                let metadata_limit = (size <= MAX_METADATA_BYTES).then_some(size);
                let (actual, metadata) = hash_entry(entry, size, metadata_limit)?;
                if actual != digest {
                    return Err(ProviderFailure::Archive);
                }
                Ok(ArchiveEntry::Blob(
                    path,
                    Blob {
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
