//! Strict tar archive inventory and path validation.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use tar::{Archive, EntryType};

use super::super::{ProviderFailure, valid_hex};
use super::MAX_ARCHIVE_BYTES;

pub(super) fn unpack(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, ProviderFailure> {
    let mut archive = Archive::new(Cursor::new(bytes));
    let mut files = HashMap::new();
    let mut directories = HashSet::new();
    let mut total_size = 0_u64;
    for entry in archive.entries().map_err(|_| ProviderFailure::Archive)? {
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        let path = entry
            .path()
            .map_err(|_| ProviderFailure::Archive)?
            .to_str()
            .ok_or(ProviderFailure::Archive)?
            .trim_end_matches('/')
            .to_owned();
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.chars().any(char::is_control)
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ProviderFailure::Archive);
        }
        match entry.header().entry_type() {
            EntryType::Directory => {
                if !["blobs", "blobs/sha256"].contains(&path.as_str())
                    || !directories.insert(path.clone())
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            EntryType::Regular => {
                let size = entry.size();
                total_size = total_size
                    .checked_add(size)
                    .ok_or(ProviderFailure::Archive)?;
                if size > MAX_ARCHIVE_BYTES as u64 || total_size > MAX_ARCHIVE_BYTES as u64 {
                    return Err(ProviderFailure::Archive);
                }
                if !allowed_file_path(&path) {
                    return Err(ProviderFailure::Archive);
                }
                let capacity = usize::try_from(size).map_err(|_| ProviderFailure::Archive)?;
                let mut data = Vec::with_capacity(capacity);
                entry
                    .read_to_end(&mut data)
                    .map_err(|_| ProviderFailure::Archive)?;
                if data.len() as u64 != size || files.insert(path, data).is_some() {
                    return Err(ProviderFailure::Archive);
                }
            }
            _ => return Err(ProviderFailure::Archive),
        }
        if files.len() > 6 || directories.len() > 2 {
            return Err(ProviderFailure::Archive);
        }
    }
    if files.len() != 6 || directories.len() != 2 {
        return Err(ProviderFailure::Archive);
    }
    for fixed in ["oci-layout", "index.json", "manifest.json"] {
        if !files.contains_key(fixed) {
            return Err(ProviderFailure::Archive);
        }
    }
    Ok(files)
}

fn allowed_file_path(path: &str) -> bool {
    ["oci-layout", "index.json", "manifest.json"].contains(&path)
        || path
            .strip_prefix("blobs/sha256/")
            .is_some_and(|digest| valid_hex(digest, 64))
}
