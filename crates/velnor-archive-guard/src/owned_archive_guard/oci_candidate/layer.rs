// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};

use super::{
    MAX_OCI_CANDIDATE_LAYER_MEMBERS, MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES, OciCandidateError,
    Result,
    archive::{Blob, OciArchive},
};

const LAYER_PATH_LIMIT: usize = 4096;
const TAR_BLOCK: usize = 512;
const TAR_EXTENSION_LIMIT: u64 = 1024 * 1024;
const TAR_EXTENSION_TOTAL_LIMIT: u64 = 4 * 1024 * 1024;

pub(super) struct LayerDescriptor {
    pub(super) digest: String,
    pub(super) size: u64,
    pub(super) media_type: String,
}

pub(super) fn verify_layers(
    archive: &mut OciArchive,
    layers: &[LayerDescriptor],
    expected_diff_ids: &[&str],
) -> Result<()> {
    if layers.len() != expected_diff_ids.len() {
        return Err(OciCandidateError("oci_diff_ids"));
    }
    let mut total_size = 0_u64;
    let mut total_members = 0_usize;
    for (layer, expected) in layers.iter().zip(expected_diff_ids) {
        let blob = archive
            .blobs
            .get(&layer.digest)
            .ok_or(OciCandidateError("oci_layer_missing"))?;
        if blob.size != layer.size {
            return Err(OciCandidateError("oci_layer_size"));
        }
        let (actual, size, members) = inspect_layer(
            &mut archive.file,
            blob,
            layer.media_type.ends_with("+gzip"),
            total_size,
            total_members,
        )?;
        if format!("sha256:{actual}") != *expected {
            return Err(OciCandidateError("oci_diff_id_mismatch"));
        }
        total_size = total_size
            .checked_add(size)
            .filter(|value| *value <= MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES)
            .ok_or(OciCandidateError("oci_layer_total_size"))?;
        total_members = total_members
            .checked_add(members)
            .filter(|value| *value <= MAX_OCI_CANDIDATE_LAYER_MEMBERS)
            .ok_or(OciCandidateError("oci_layer_member_total"))?;
    }
    Ok(())
}

fn inspect_layer(
    file: &mut File,
    blob: &Blob,
    gzip: bool,
    previous_bytes: u64,
    previous_members: usize,
) -> Result<(String, u64, usize)> {
    file.seek(SeekFrom::Start(blob.offset))
        .map_err(|_| OciCandidateError("oci_layer_seek"))?;
    scan_extensions(
        file,
        blob,
        gzip,
        MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES - previous_bytes,
    )?;
    let source = open_layer(file, blob, gzip)?;
    let mut reader = HashLimitReader::new(
        source,
        MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES - previous_bytes,
    );
    let names = {
        let mut archive = tar::Archive::new(&mut reader);
        let entries = archive
            .entries()
            .map_err(|_| OciCandidateError("oci_layer_tar"))?;
        let mut names = BTreeSet::new();
        for entry in entries {
            if names.len() + previous_members >= MAX_OCI_CANDIDATE_LAYER_MEMBERS {
                return Err(OciCandidateError("oci_layer_member_limit"));
            }
            let entry = entry.map_err(|_| OciCandidateError("oci_layer_entry"))?;
            let name = entry
                .path()
                .map_err(|_| OciCandidateError("oci_layer_path"))?
                .to_str()
                .ok_or(OciCandidateError("oci_layer_path_utf8"))?
                .to_owned();
            let normalized = validate_layer_path(&name)?;
            if !names.insert(normalized) {
                return Err(OciCandidateError("oci_layer_duplicate_path"));
            }
        }
        names
    };
    let mut sink = vec![0_u8; 64 * 1024];
    while reader
        .read(&mut sink)
        .map_err(|_| OciCandidateError("oci_layer_read"))?
        != 0
    {}
    Ok((reader.hex_digest(), reader.size, names.len()))
}

fn open_layer<'a>(file: &'a mut File, blob: &Blob, gzip: bool) -> Result<Box<dyn Read + 'a>> {
    file.seek(SeekFrom::Start(blob.offset))
        .map_err(|_| OciCandidateError("oci_layer_seek"))?;
    let limited = file.take(blob.size);
    Ok(if gzip {
        Box::new(MultiGzDecoder::new(limited))
    } else {
        Box::new(limited)
    })
}

fn scan_extensions(file: &mut File, blob: &Blob, gzip: bool, limit: u64) -> Result<()> {
    let mut source = HashLimitReader::new(open_layer(file, blob, gzip)?, limit);
    let mut total = 0_u64;
    let mut header = [0_u8; TAR_BLOCK];
    loop {
        let mut filled = 0;
        while filled < header.len() {
            let count = source
                .read(&mut header[filled..])
                .map_err(|_| OciCandidateError("oci_layer_extension_read"))?;
            if count == 0 {
                return Err(OciCandidateError(if filled == 0 {
                    "oci_layer_terminator_missing"
                } else {
                    "oci_layer_extension_truncated_header"
                }));
            }
            filled += count;
        }
        if header.iter().all(|byte| *byte == 0) {
            return validate_terminator_and_padding(&mut source);
        }
        let size = tar_size(&header[124..136])?;
        if matches!(header[156], b'x' | b'g' | b'L' | b'K') {
            if size > TAR_EXTENSION_LIMIT {
                return Err(OciCandidateError("oci_layer_extension_limit"));
            }
            total = total
                .checked_add(size)
                .filter(|value| *value <= TAR_EXTENSION_TOTAL_LIMIT)
                .ok_or(OciCandidateError("oci_layer_extension_total_limit"))?;
        }
        let padded = size
            .checked_add(TAR_BLOCK as u64 - 1)
            .ok_or(OciCandidateError("oci_layer_tar_size"))?
            / TAR_BLOCK as u64
            * TAR_BLOCK as u64;
        skip_bytes(&mut source, padded)?;
    }
}

fn validate_terminator_and_padding<R: Read>(source: &mut R) -> Result<()> {
    let mut second = [0_u8; TAR_BLOCK];
    let mut filled = 0;
    while filled < second.len() {
        let count = source
            .read(&mut second[filled..])
            .map_err(|_| OciCandidateError("oci_layer_terminator_read"))?;
        if count == 0 {
            return Err(OciCandidateError("oci_layer_terminator_truncated"));
        }
        filled += count;
    }
    if second.iter().any(|byte| *byte != 0) {
        return Err(OciCandidateError("oci_layer_terminator_invalid"));
    }

    let mut padding = vec![0_u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut padding)
            .map_err(|_| OciCandidateError("oci_layer_terminator_read"))?;
        if count == 0 {
            return Ok(());
        }
        if padding[..count].iter().any(|byte| *byte != 0) {
            return Err(OciCandidateError("oci_layer_trailing_data"));
        }
    }
}

fn skip_bytes<R: Read>(reader: &mut R, mut remaining: u64) -> Result<()> {
    let mut buffer = vec![0_u8; 64 * 1024];
    while remaining > 0 {
        let amount = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| OciCandidateError("oci_layer_tar_size"))?;
        let count = reader
            .read(&mut buffer[..amount])
            .map_err(|_| OciCandidateError("oci_layer_extension_read"))?;
        if count == 0 {
            return Err(OciCandidateError("oci_layer_extension_truncated_body"));
        }
        remaining -= count as u64;
    }
    Ok(())
}

fn tar_size(field: &[u8]) -> Result<u64> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut value = 0_u64;
        for (index, byte) in field.iter().enumerate() {
            let byte = if index == 0 { byte & 0x7f } else { *byte };
            value = value
                .checked_mul(256)
                .and_then(|next| next.checked_add(u64::from(byte)))
                .ok_or(OciCandidateError("oci_layer_tar_size"))?;
        }
        return Ok(value);
    }
    field
        .iter()
        .copied()
        .take_while(|byte| *byte != 0 && *byte != b' ')
        .try_fold(0_u64, |value, byte| {
            if !(b'0'..=b'7').contains(&byte) {
                return Err(OciCandidateError("oci_layer_tar_size"));
            }
            value
                .checked_mul(8)
                .and_then(|next| next.checked_add(u64::from(byte - b'0')))
                .ok_or(OciCandidateError("oci_layer_tar_size"))
        })
}

fn validate_layer_path(value: &str) -> Result<String> {
    if value.len() > LAYER_PATH_LIMIT || value.starts_with('/') || value.contains('\0') {
        return Err(OciCandidateError("oci_layer_path_unsafe"));
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(OciCandidateError("oci_layer_path_unsafe")),
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return Ok(".".to_owned());
    }
    Ok(parts.join("/"))
}

struct HashLimitReader<R> {
    inner: R,
    hasher: Sha256,
    size: u64,
    limit: u64,
}

impl<R> HashLimitReader<R> {
    fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            size: 0,
            limit,
        }
    }
    fn hex_digest(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let digest = self.hasher.clone().finalize();
        let mut encoded = String::with_capacity(64);
        for byte in digest {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        encoded
    }
}

impl<R: Read> Read for HashLimitReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.size = self
            .size
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("layer byte overflow"))?;
        if self.size > self.limit {
            return Err(io::Error::other("layer uncompressed size limit"));
        }
        self.hasher.update(&buffer[..count]);
        Ok(count)
    }
}
