// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashSet};
use std::io::Read;

use sha2::{Digest, Sha256};

use super::hashing::{Hashing, hex, is_limit};
use super::paths::{joined, normalize, text};
use super::tar::{Entry, Kind, TarError, TarReader};
use crate::error::CiError;

pub const MAX_ARCHIVE_BYTES: u64 = 4 << 30;
pub const MAX_BLOB_BYTES: u64 = 1 << 30;
const MAX_MEMBERS: usize = 128;
const MAX_BLOBS: usize = 64;
const MAX_NAME: usize = 1024;
const MAX_METADATA: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub size: u64,
    pub sha: String,
    pub offset: u64,
}

#[derive(Debug)]
pub struct Scanned {
    pub layout: Vec<u8>,
    pub index: Vec<u8>,
    pub blobs: BTreeMap<String, Blob>,
    pub archive_sha: String,
    pub archive_size: u64,
}

enum Slot {
    Directory,
    Blob(String),
    Metadata(Vec<u8>),
}

struct Admission {
    seen: HashSet<Vec<u8>>,
    content_size: u64,
    blobs: usize,
}

impl Admission {
    fn admit(&mut self, entry: &Entry) -> Result<Slot, CiError> {
        if self.seen.len() >= MAX_MEMBERS {
            return Err(CiError::new("OCI archive has too many entries."));
        }
        let name = safe_member(&entry.name)?;
        if name.len() > MAX_NAME {
            return Err(CiError::new("OCI archive path exceeds its size limit."));
        }
        if !self.seen.insert(name.clone()) {
            return Err(CiError::new(format!(
                "Duplicate archive path: {}",
                text(&name)
            )));
        }
        match entry.kind {
            Kind::Dir if name == b"blobs" || name == b"blobs/sha256" => return Ok(Slot::Directory),
            Kind::Dir => {
                return Err(CiError::new(format!(
                    "Unexpected OCI archive directory: {}",
                    text(&name)
                )));
            }
            Kind::Other => {
                return Err(CiError::new(format!(
                    "OCI archive entry is not a regular file: {}",
                    text(&name)
                )));
            }
            Kind::File => {}
        }
        if entry.size > MAX_BLOB_BYTES {
            return Err(CiError::new("OCI archive entry exceeds its size limit."));
        }
        self.content_size += entry.size;
        if self.content_size > MAX_ARCHIVE_BYTES {
            return Err(CiError::new("OCI archive content exceeds its size limit."));
        }
        if let Some(key) = blob_key(&name) {
            if self.blobs >= MAX_BLOBS {
                return Err(CiError::new("OCI archive has too many blobs."));
            }
            self.blobs += 1;
            return Ok(Slot::Blob(key));
        }
        if name != b"oci-layout" && name != b"index.json" {
            return Err(CiError::new(format!(
                "Unexpected OCI archive path: {}",
                text(&name)
            )));
        }
        if entry.size > MAX_METADATA {
            return Err(CiError::new(format!(
                "OCI metadata file is too large: {}",
                text(&name)
            )));
        }
        Ok(Slot::Metadata(name))
    }
}

pub fn scan(reader: impl Read) -> Result<Scanned, CiError> {
    let mut tar = TarReader::new(Hashing::new(reader, MAX_ARCHIVE_BYTES));
    let mut admission = Admission {
        seen: HashSet::new(),
        content_size: 0,
        blobs: 0,
    };
    let mut blobs: BTreeMap<String, Blob> = BTreeMap::new();
    let mut layout = None;
    let mut index = None;
    loop {
        let entry = match tar.next_entry() {
            Ok(Some(entry)) => entry,
            Ok(None) if tar.position() == 0 => return Err(unreadable()),
            Ok(None) => break,
            Err(error) => return Err(tar_error(error)),
        };
        match admission.admit(&entry)? {
            Slot::Directory => {}
            Slot::Blob(key) => {
                let mut sha = Sha256::new();
                tar.read_data(&mut |chunk| sha.update(chunk))
                    .map_err(tar_error)?;
                let blob = Blob {
                    size: entry.size,
                    sha: hex(&sha.finalize()),
                    offset: entry.offset,
                };
                blobs.insert(key, blob);
            }
            Slot::Metadata(name) => {
                let mut data = Vec::new();
                tar.read_data(&mut |chunk| data.extend_from_slice(chunk))
                    .map_err(tar_error)?;
                if name == b"oci-layout" {
                    layout = Some(data);
                } else {
                    index = Some(data);
                }
            }
        }
    }
    let (Some(layout), Some(index)) = (layout, index) else {
        return Err(CiError::new(
            "OCI archive must contain oci-layout and index.json.",
        ));
    };
    let (archive_sha, archive_size) = drain(tar.into_inner())?;
    Ok(Scanned {
        layout,
        index,
        blobs,
        archive_sha,
        archive_size,
    })
}

fn drain(mut rest: Hashing<impl Read>) -> Result<(String, u64), CiError> {
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        match rest.read(&mut buffer) {
            Ok(0) => return Ok(rest.finish()),
            Ok(_) => {}
            Err(error) if is_limit(&error) => {
                return Err(CiError::new("OCI archive content exceeds its size limit."));
            }
            Err(_) => return Err(unreadable()),
        }
    }
}

fn blob_key(name: &[u8]) -> Option<String> {
    let hex = name.strip_prefix(b"blobs/sha256/")?;
    let hex = std::str::from_utf8(hex).ok()?;
    super::json::is_hex(hex, 64).then(|| hex.to_owned())
}

fn safe_member(name: &[u8]) -> Result<Vec<u8>, CiError> {
    let (absolute, parts) = normalize(name);
    if absolute || parts.contains(&&b".."[..]) || parts.is_empty() || joined(&parts) != name {
        return Err(CiError::new(
            "OCI archive contains a path that is not safe.",
        ));
    }
    Ok(name.to_vec())
}

fn unreadable() -> CiError {
    CiError::new("Cannot read OCI archive.")
}

fn tar_error(error: TarError) -> CiError {
    match error {
        TarError::ExtensionSize => CiError::new("Tar extension header exceeds its size limit."),
        TarError::ExtensionTotal => {
            CiError::new("Tar extension headers exceed their total size limit.")
        }
        TarError::MemberSize => CiError::new("Tar archive has an incorrect member size."),
        TarError::Io(error) if is_limit(&error) => {
            CiError::new("OCI archive content exceeds its size limit.")
        }
        TarError::Io(_) | TarError::Format(_) => unreadable(),
    }
}
