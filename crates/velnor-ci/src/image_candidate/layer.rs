// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::io::{self, BufReader, Read};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::members::GzipMembers;

use super::hashing::{Hashing, is_limit};
use super::paths::{joined, normalize, text};
use super::tar::{TarError, TarReader};
use crate::error::CiError;

const NOT_TAR: &str = "OCI layer is not a tar archive.";
const OVER_SIZE: &str = "OCI image layers exceed their uncompressed size limit.";
const DRAIN: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub diff_id: String,
    pub size: u64,
    pub members: usize,
    pub blob_sha: String,
}

#[derive(Debug)]
pub struct Budget {
    bytes: AtomicU64,
    members: AtomicUsize,
}

impl Budget {
    #[must_use]
    pub fn new(bytes: u64, members: usize) -> Self {
        Self {
            bytes: AtomicU64::new(bytes),
            members: AtomicUsize::new(members),
        }
    }

    fn take_bytes(&self, count: u64) -> bool {
        let left = self
            .bytes
            .update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.saturating_sub(count)
            });
        left >= count
    }

    fn take_member(&self) -> bool {
        self.members
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            })
            .is_ok()
    }
}

struct Metered<'a, R> {
    inner: R,
    budget: &'a Budget,
}

impl<R: Read> Read for Metered<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buf)?;
        if self.budget.take_bytes(count as u64) {
            Ok(count)
        } else {
            Err(io::Error::new(io::ErrorKind::FileTooLarge, "size limit"))
        }
    }
}

pub fn scan(
    source: impl Read,
    gzip: bool,
    byte_limit: u64,
    member_limit: usize,
) -> Result<Layer, CiError> {
    scan_shared(source, gzip, &Budget::new(byte_limit, member_limit))
}

pub fn scan_shared(source: impl Read, gzip: bool, budget: &Budget) -> Result<Layer, CiError> {
    if gzip {
        let mut compressed = Hashing::new(source, u64::MAX);
        let decoder = GzipMembers::new(BufReader::with_capacity(DRAIN, &mut compressed));
        let (diff_id, size, members) = scan_tar(decoder, budget)?;
        let (blob_sha, _) = compressed.finish();
        Ok(Layer {
            diff_id,
            size,
            members,
            blob_sha,
        })
    } else {
        let (diff_id, size, members) = scan_tar(source, budget)?;
        Ok(Layer {
            blob_sha: diff_id.clone(),
            diff_id,
            size,
            members,
        })
    }
}

fn scan_tar(stream: impl Read, budget: &Budget) -> Result<(String, u64, usize), CiError> {
    let metered = Metered {
        inner: stream,
        budget,
    };
    let mut tar = TarReader::new(Hashing::new(metered, u64::MAX));
    let empty = || CiError::new(NOT_TAR);
    let mut paths: HashSet<Vec<u8>> = HashSet::new();
    loop {
        match tar.next_entry() {
            Ok(Some(entry)) => {
                if !budget.take_member() {
                    return Err(CiError::new("OCI layer has too many paths."));
                }
                let normalized = layer_path(&entry.name)?;
                if !paths.insert(normalized.clone()) {
                    return Err(CiError::new(format!(
                        "OCI layer repeats a path: {}",
                        text(&normalized)
                    )));
                }
            }
            Ok(None) if tar.position() == 0 => return Err(empty()),
            Ok(None) => break,
            Err(error) => return Err(tar_error(error)),
        }
    }
    let mut rest = tar.into_inner();
    let mut buffer = vec![0u8; DRAIN];
    loop {
        match rest.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => return Err(io_error(&error)),
        }
    }
    let (diff_id, size) = rest.finish();
    Ok((diff_id, size, paths.len()))
}

fn layer_path(name: &[u8]) -> Result<Vec<u8>, CiError> {
    let (absolute, parts) = normalize(name);
    if absolute || parts.contains(&&b".."[..]) {
        return Err(CiError::new("OCI layer has a path that is not safe."));
    }
    if parts.is_empty() {
        return Ok(b".".to_vec());
    }
    Ok(joined(&parts))
}

fn io_error(error: &io::Error) -> CiError {
    if is_limit(error) {
        CiError::new(OVER_SIZE)
    } else {
        CiError::new(NOT_TAR)
    }
}

fn tar_error(error: TarError) -> CiError {
    match error {
        TarError::ExtensionSize => CiError::new("Tar extension header exceeds its size limit."),
        TarError::ExtensionTotal => {
            CiError::new("Tar extension headers exceed their total size limit.")
        }
        TarError::MemberSize => CiError::new("Tar archive has an incorrect member size."),
        TarError::Io(error) => io_error(&error),
        TarError::Format(_) => CiError::new(NOT_TAR),
    }
}
