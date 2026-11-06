// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
};

use super::{ASSET_LIMIT, Admission, FILE_LIMIT, JSON_LIMIT, safe_file, sha256};
use crate::{OrchestratorError, internal::internal};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub(super) const RAW_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const ENTRY_LIMIT: usize = FILE_LIMIT + 1;
const DIRECTORY_LIMIT: u64 = 64 * 1024;
const TOTAL_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

pub(super) fn encode_bundle(
    admission: &[u8],
    files: BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, bytes) in
        std::iter::once(("admission.json".to_owned(), admission.to_vec())).chain(files)
    {
        archive
            .start_file(name, options)
            .map_err(|_| internal("homebrew_handoff_zip_encode"))?;
        archive
            .write_all(&bytes)
            .map_err(|_| internal("homebrew_handoff_zip_encode"))?;
    }
    let bytes = archive
        .finish()
        .map_err(|_| internal("homebrew_handoff_zip_encode"))?
        .into_inner();
    open_bundle(&bytes)?;
    Ok(bytes)
}

pub(super) fn open_bundle(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>, OrchestratorError> {
    let count = preflight_directory(bytes)?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| internal("homebrew_handoff_zip"))?;
    if archive.is_empty() || archive.len() > ENTRY_LIMIT || archive.len() != count {
        return Err(internal("homebrew_handoff_zip_entries"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| internal("homebrew_handoff_zip_entry"))?;
        let name = file
            .name()
            .map_err(|_| internal("homebrew_handoff_zip_name"))?
            .into_owned();
        let limit = if matches!(
            name.as_str(),
            "admission.json" | "A/release.json" | "D/release.json"
        ) {
            JSON_LIMIT
        } else {
            ASSET_LIMIT
        };
        total = total
            .checked_add(file.size())
            .ok_or_else(|| internal("homebrew_handoff_zip_size"))?;
        if (name != "admission.json" && !safe_file(&name))
            || !names.insert(name)
            || file.is_dir()
            || file
                .unix_mode()
                .is_none_or(|mode| mode & 0o170_000 != 0o100_000)
            || file.size() == 0
            || file.size() > limit
            || file.compressed_size() > RAW_LIMIT
            || file.compression() != zip::CompressionMethod::Stored
            || file.compressed_size() != file.size()
            || total > TOTAL_LIMIT
        {
            return Err(internal("homebrew_handoff_zip_inventory"));
        }
    }
    Ok(archive)
}

fn preflight_directory(bytes: &[u8]) -> Result<usize, OrchestratorError> {
    if bytes.len() < 22 || bytes.len() as u64 > RAW_LIMIT {
        return Err(internal("homebrew_handoff_zip_size"));
    }
    let eocd = &bytes[bytes.len() - 22..];
    let word = |offset: usize| u16::from_le_bytes([eocd[offset], eocd[offset + 1]]);
    let dword = |offset: usize| {
        u32::from_le_bytes([
            eocd[offset],
            eocd[offset + 1],
            eocd[offset + 2],
            eocd[offset + 3],
        ])
    };
    let count = word(10);
    let directory_size = u64::from(dword(12));
    let directory_offset = u64::from(dword(16));
    if eocd[..4] != [0x50, 0x4b, 0x05, 0x06]
        || word(4) != 0
        || word(6) != 0
        || word(8) != count
        || count == 0
        || usize::from(count) > ENTRY_LIMIT
        || word(20) != 0
        || directory_size > DIRECTORY_LIMIT
        || directory_offset + directory_size != bytes.len() as u64 - 22
    {
        return Err(internal("homebrew_handoff_zip_directory"));
    }
    Ok(usize::from(count))
}

pub(super) fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| internal("homebrew_handoff_zip_missing"))?;
    let size = file.size();
    if size == 0 || size > limit {
        return Err(internal("homebrew_handoff_zip_size"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| internal("homebrew_handoff_zip_content"))?;
    if bytes.len() as u64 != size {
        return Err(internal("homebrew_handoff_zip_size"));
    }
    Ok(bytes)
}

pub(super) fn read_admitted_files(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    admission: &Admission,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let expected = std::iter::once("admission.json".to_owned())
        .chain(admission.files.keys().cloned())
        .collect::<BTreeSet<_>>();
    let actual = archive
        .file_names()
        .map(|name| name.map(std::borrow::Cow::into_owned))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| internal("homebrew_handoff_zip_name"))?;
    if actual != expected {
        return Err(internal("homebrew_handoff_zip_inventory"));
    }
    let mut files = BTreeMap::new();
    for (name, identity) in &admission.files {
        let bytes = read_entry(archive, name, identity.size)?;
        if bytes.len() as u64 != identity.size || sha256(&bytes) != identity.sha256 {
            return Err(internal("homebrew_handoff_file_digest"));
        }
        files.insert(name.clone(), bytes);
    }
    Ok(files)
}

pub(super) fn admitted_bytes(
    files: &BTreeMap<String, Vec<u8>>,
    name: &str,
    admission: &Admission,
    limit: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    let identity = admission
        .files
        .get(name)
        .ok_or_else(|| internal("homebrew_handoff_file_not_admitted"))?;
    let bytes = files
        .get(name)
        .ok_or_else(|| internal("homebrew_handoff_file_not_admitted"))?;
    if identity.size > limit
        || bytes.len() as u64 != identity.size
        || sha256(bytes) != identity.sha256
    {
        return Err(internal("homebrew_handoff_file_digest"));
    }
    Ok(bytes.clone())
}
