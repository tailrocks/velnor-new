// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::time::Instant;

use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};
use velnor_runner_image_manifest::OciDescriptor;

use super::ProviderFailure;
use super::model::Blob;

const BYTE_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
const MEMBER_LIMIT: usize = 1_000_000;
const BLOCK: usize = 512;
const EXTENSION_LIMIT: u64 = 1024 * 1024;
const EXTENSION_TOTAL_LIMIT: u64 = 4 * 1024 * 1024;

pub(super) fn verify(
    held: &File,
    blobs: &HashMap<String, Blob>,
    layers: &[OciDescriptor],
    diff_ids: &[String],
    deadline: Instant,
) -> Result<(), ProviderFailure> {
    let result = verify_inner(held, blobs, layers, diff_ids, deadline);
    if result.is_err() && Instant::now() >= deadline {
        Err(ProviderFailure::Timeout)
    } else {
        result
    }
}

fn verify_inner(
    held: &File,
    blobs: &HashMap<String, Blob>,
    layers: &[OciDescriptor],
    diff_ids: &[String],
    deadline: Instant,
) -> Result<(), ProviderFailure> {
    if layers.len() != diff_ids.len() {
        return Err(ProviderFailure::Archive);
    }
    let mut total_bytes = 0_u64;
    let mut total_members = 0_usize;
    for (descriptor, expected) in layers.iter().zip(diff_ids) {
        if !velnor_runner_image_manifest::is_oci_layer_media_type(&descriptor.media_type) {
            return Err(ProviderFailure::Archive);
        }
        let path = format!("blobs/sha256/{}", descriptor.digest);
        let blob = blobs.get(&path).ok_or(ProviderFailure::Archive)?;
        if blob.size != descriptor.size {
            return Err(ProviderFailure::Archive);
        }
        let available = BYTE_LIMIT
            .checked_sub(total_bytes)
            .ok_or(ProviderFailure::Archive)?;
        let gzip = descriptor.media_type.ends_with("+gzip");
        scan_framing(held, blob, gzip, available, deadline)?;
        let reader = open_layer(held, blob, gzip, deadline)?;
        let mut reader = Limited::new(reader, available, deadline);
        let mut paths = BTreeSet::new();
        {
            let mut archive = tar::Archive::new(&mut reader);
            let entries = archive.entries().map_err(|_| ProviderFailure::Archive)?;
            for entry in entries {
                ensure_deadline(deadline)?;
                if total_members + paths.len() >= MEMBER_LIMIT {
                    return Err(ProviderFailure::Archive);
                }
                let entry = entry.map_err(|_| ProviderFailure::Archive)?;
                let path = entry.path().map_err(|_| ProviderFailure::Archive)?;
                let value = path.to_str().ok_or(ProviderFailure::Archive)?;
                if !paths.insert(normalize_path(value)?) {
                    return Err(ProviderFailure::Archive);
                }
            }
        }
        let mut sink = vec![0_u8; 64 * 1024];
        while reader
            .read(&mut sink)
            .map_err(|_| ProviderFailure::Archive)?
            != 0
        {}
        let actual = format!("sha256:{}", super::hex(&reader.hasher.finalize()));
        if &actual != expected {
            return Err(ProviderFailure::Archive);
        }
        total_bytes = total_bytes
            .checked_add(reader.size)
            .ok_or(ProviderFailure::Archive)?;
        total_members = total_members
            .checked_add(paths.len())
            .ok_or(ProviderFailure::Archive)?;
    }
    ensure_deadline(deadline)
}

fn open_layer(
    held: &File,
    blob: &Blob,
    gzip: bool,
    deadline: Instant,
) -> Result<Box<dyn Read>, ProviderFailure> {
    ensure_deadline(deadline)?;
    let mut file = held.try_clone().map_err(|_| ProviderFailure::Archive)?;
    file.seek(SeekFrom::Start(
        blob.offset.ok_or(ProviderFailure::Archive)?,
    ))
    .map_err(|_| ProviderFailure::Archive)?;
    let bounded = file.take(blob.size);
    Ok(if gzip {
        Box::new(MultiGzDecoder::new(bounded))
    } else {
        Box::new(bounded)
    })
}

fn scan_framing(
    held: &File,
    blob: &Blob,
    gzip: bool,
    limit: u64,
    deadline: Instant,
) -> Result<(), ProviderFailure> {
    let mut source = Limited::new(open_layer(held, blob, gzip, deadline)?, limit, deadline);
    let mut header = [0_u8; BLOCK];
    let mut extension_total = 0_u64;
    loop {
        ensure_deadline(deadline)?;
        source
            .read_exact(&mut header)
            .map_err(|_| ProviderFailure::Archive)?;
        if header.iter().all(|byte| *byte == 0) {
            source
                .read_exact(&mut header)
                .map_err(|_| ProviderFailure::Archive)?;
            if header.iter().any(|byte| *byte != 0) {
                return Err(ProviderFailure::Archive);
            }
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                let count = source
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
        let size = super::archive_read::strict_tar_size(&header[124..136])?;
        if matches!(header[156], b'x' | b'g' | b'L' | b'K') {
            if size > EXTENSION_LIMIT {
                return Err(ProviderFailure::Archive);
            }
            extension_total = extension_total
                .checked_add(size)
                .filter(|value| *value <= EXTENSION_TOTAL_LIMIT)
                .ok_or(ProviderFailure::Archive)?;
        }
        let mut remaining = size
            .checked_add((BLOCK - 1) as u64)
            .ok_or(ProviderFailure::Archive)?
            / BLOCK as u64
            * BLOCK as u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        while remaining > 0 {
            let amount = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| ProviderFailure::Archive)?;
            let count = source
                .read(&mut buffer[..amount])
                .map_err(|_| ProviderFailure::Archive)?;
            if count == 0 {
                return Err(ProviderFailure::Archive);
            }
            remaining -= count as u64;
        }
    }
}

fn normalize_path(value: &str) -> Result<String, ProviderFailure> {
    if value.len() > 4096 || value.starts_with('/') || value.contains('\0') {
        return Err(ProviderFailure::Archive);
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(ProviderFailure::Archive),
            part => parts.push(part),
        }
    }
    Ok(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}

fn ensure_deadline(deadline: Instant) -> Result<(), ProviderFailure> {
    if Instant::now() >= deadline {
        Err(ProviderFailure::Timeout)
    } else {
        Ok(())
    }
}

struct Limited<R> {
    inner: R,
    hasher: Sha256,
    size: u64,
    limit: u64,
    deadline: Instant,
}

impl<R> Limited<R> {
    fn new(inner: R, limit: u64, deadline: Instant) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            size: 0,
            limit,
            deadline,
        }
    }
}

impl<R: Read> Read for Limited<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        ensure_deadline(self.deadline).map_err(|_| io::Error::other("deadline"))?;
        let count = self.inner.read(buffer)?;
        ensure_deadline(self.deadline).map_err(|_| io::Error::other("deadline"))?;
        self.size = self
            .size
            .checked_add(count as u64)
            .filter(|value| *value <= self.limit)
            .ok_or_else(|| io::Error::other("layer size"))?;
        self.hasher.update(&buffer[..count]);
        Ok(count)
    }
}
