//! Strict tar member extraction with byte, count, path, and duplicate bounds.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::super::hash::{is_lower_hex, lowercase_hex};
use super::{MAX_ARCHIVE_BYTES, MAX_ARCHIVE_MEMBERS, MAX_MEMBER_BYTES};

pub(super) fn read_archive(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, HostError> {
    let mut archive = tar::Archive::new(Cursor::new(bytes));
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut total = 0_usize;
    for (index, entry) in archive
        .entries()
        .map_err(|_| HostError::Identity)?
        .enumerate()
    {
        if index >= MAX_ARCHIVE_MEMBERS {
            return Err(HostError::Frame);
        }
        let entry = entry.map_err(|_| HostError::Identity)?;
        let path = entry.path().map_err(|_| HostError::Identity)?;
        let name = path.to_str().ok_or(HostError::Identity)?.to_owned();
        if !names.insert(name.clone()) {
            return Err(HostError::Identity);
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            if !matches!(name.as_str(), "blobs" | "blobs/sha256") || entry.size() != 0 {
                return Err(HostError::Identity);
            }
            directories.insert(name);
            continue;
        }
        if !kind.is_file()
            || entry.size() > u64::try_from(MAX_MEMBER_BYTES).map_err(|_| HostError::Frame)?
            || !valid_file_path(&name)
        {
            return Err(HostError::Identity);
        }
        let size = usize::try_from(entry.size()).map_err(|_| HostError::Frame)?;
        total = total
            .checked_add(size)
            .filter(|value| *value <= MAX_ARCHIVE_BYTES)
            .ok_or(HostError::Frame)?;
        let mut content = Vec::with_capacity(size);
        entry
            .take(
                u64::try_from(size)
                    .map_err(|_| HostError::Frame)?
                    .saturating_add(1),
            )
            .read_to_end(&mut content)
            .map_err(|_| HostError::Identity)?;
        if content.len() != size {
            return Err(HostError::Identity);
        }
        verify_blob_name(&name, &content)?;
        files.insert(name, content);
    }
    verify_layout(&directories, &files)?;
    Ok(files)
}

fn valid_file_path(name: &str) -> bool {
    matches!(name, "oci-layout" | "index.json" | "manifest.json")
        || name
            .strip_prefix("blobs/sha256/")
            .is_some_and(|digest| is_lower_hex(digest, 64))
}

fn verify_blob_name(name: &str, bytes: &[u8]) -> Result<(), HostError> {
    if !name.starts_with("blobs/") {
        return Ok(());
    }
    let digest = name
        .strip_prefix("blobs/sha256/")
        .ok_or(HostError::Identity)?;
    if !is_lower_hex(digest, 64) || lowercase_hex(&Sha256::digest(bytes)) != digest {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn verify_layout(
    directories: &BTreeSet<String>,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), HostError> {
    let expected = BTreeSet::from(["blobs".to_owned(), "blobs/sha256".to_owned()]);
    let required = ["oci-layout", "index.json", "manifest.json"];
    if directories != &expected || required.iter().any(|name| !files.contains_key(*name)) {
        return Err(HostError::Identity);
    }
    Ok(())
}
