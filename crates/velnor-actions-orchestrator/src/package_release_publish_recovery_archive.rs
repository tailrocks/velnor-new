//! Bounded admission of the signed package artifact archive.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read, Write},
    path::Path,
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    PackageReleasePlan, package_release::PackageReleaseReceipt, strict_json::parse_strict_json,
};

use super::super::github::sha256;
use crate::{OrchestratorError, internal::internal};

const MAX_RECEIPT_BYTES: u64 = 1024 * 1024;
const MAX_RECEIPT_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub(super) fn extract_signed_assets(
    archive_bytes: &[u8],
    destination: &Path,
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive_bytes))
        .map_err(|_| internal("package_recovery_zip_invalid"))?;
    let receipt_bytes = read_receipt_bytes(&mut archive)?;
    let receipt = parse_receipt(&receipt_bytes, plan)?;
    let expected_sizes = expected_archive_sizes(&receipt, receipt_bytes.len(), archive.len())?;
    let entries = admit_archive_entries(&mut archive, &expected_sizes)?;
    extract_archive_entries(
        &mut archive,
        destination,
        &entries,
        &receipt_bytes,
        &receipt,
    )?;
    Ok(receipt)
}

fn read_receipt_bytes<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<Vec<u8>, OrchestratorError> {
    let file = archive
        .by_name("release.json")
        .map_err(|_| internal("package_recovery_receipt_missing"))?;
    if file.is_dir() || file.size() == 0 || file.size() > MAX_RECEIPT_BYTES {
        return Err(internal("package_recovery_receipt_size"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| internal("package_recovery_receipt_read"))?;
    Ok(bytes)
}

fn expected_archive_sizes(
    receipt: &PackageReleaseReceipt,
    receipt_size: usize,
    archive_entries: usize,
) -> Result<BTreeMap<String, Option<u64>>, OrchestratorError> {
    let mut expected: BTreeMap<String, Option<u64>> = receipt
        .assets
        .iter()
        .map(|asset| (asset.name.clone(), Some(asset.size)))
        .collect();
    expected.insert("release.json".to_owned(), Some(receipt_size as u64));
    expected.insert("release.json.bundle".to_owned(), None);
    if expected.len() != receipt.assets.len() + 2 || expected.len() != archive_entries {
        return Err(internal("package_recovery_zip_inventory"));
    }
    let total_declared: u64 = expected.values().filter_map(|size| *size).sum();
    if total_declared > MAX_UNPACKED_BYTES {
        return Err(internal("package_recovery_unpacked_limit"));
    }
    Ok(expected)
}

struct AdmittedArchiveEntry {
    index: usize,
    name: String,
    size: u64,
}

fn admit_archive_entries<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    expected_sizes: &BTreeMap<String, Option<u64>>,
) -> Result<Vec<AdmittedArchiveEntry>, OrchestratorError> {
    let mut observed = BTreeSet::new();
    let mut admitted = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| internal("package_recovery_zip_entry"))?;
        let name = entry
            .name()
            .map_err(|_| internal("package_recovery_zip_path"))?
            .into_owned();
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name == "."
            || name == ".."
            || entry.is_dir()
            || !observed.insert(name.clone())
        {
            return Err(internal("package_recovery_zip_path"));
        }
        let Some(expected_size) = expected_sizes.get(&name) else {
            return Err(internal("package_recovery_zip_unexpected_asset"));
        };
        if entry.size() > MAX_UNPACKED_BYTES
            || expected_size.is_some_and(|size| size != entry.size())
            || (name == "release.json.bundle" && entry.size() > MAX_RECEIPT_BUNDLE_BYTES)
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 != 0 && mode & 0o170_000 != 0o100_000)
        {
            return Err(internal("package_recovery_zip_file_type_or_size"));
        }
        admitted.push(AdmittedArchiveEntry {
            index,
            name,
            size: entry.size(),
        });
    }
    if observed.len() != expected_sizes.len() {
        return Err(internal("package_recovery_zip_inventory"));
    }
    Ok(admitted)
}

fn extract_archive_entries<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    destination: &Path,
    entries: &[AdmittedArchiveEntry],
    receipt_bytes: &[u8],
    receipt: &PackageReleaseReceipt,
) -> Result<(), OrchestratorError> {
    for admitted in entries {
        let mut entry = archive
            .by_index(admitted.index)
            .map_err(|_| internal("package_recovery_zip_entry"))?;
        let output_path = destination.join(&admitted.name);
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_path)
            .map_err(|_| internal("package_recovery_output_create"))?;
        let mut hasher = Sha256::new();
        let mut copied = 0u64;
        let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
        loop {
            let count = entry
                .read(&mut buffer)
                .map_err(|_| internal("package_recovery_zip_read"))?;
            if count == 0 {
                break;
            }
            copied = copied
                .checked_add(count as u64)
                .ok_or_else(|| internal("package_recovery_zip_size_overflow"))?;
            if copied > admitted.size || copied > MAX_UNPACKED_BYTES {
                return Err(internal("package_recovery_unpacked_limit"));
            }
            output
                .write_all(&buffer[..count])
                .map_err(|_| internal("package_recovery_output_write"))?;
            hasher.update(&buffer[..count]);
        }
        if copied != admitted.size {
            return Err(internal("package_recovery_zip_truncated"));
        }
        if admitted.name != "release.json.bundle" {
            let expected_hash = if admitted.name == "release.json" {
                sha256(receipt_bytes)
            } else {
                receipt
                    .assets
                    .iter()
                    .find(|asset| asset.name == admitted.name)
                    .map(|asset| asset.sha256.clone())
                    .ok_or_else(|| internal("package_recovery_zip_unexpected_asset"))?
            };
            if hex_digest(hasher.finalize()) != expected_hash {
                return Err(internal("package_recovery_zip_asset_digest"));
            }
        }
    }
    Ok(())
}

fn parse_receipt(
    bytes: &[u8],
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    let value: Value = parse_strict_json(
        std::str::from_utf8(bytes).map_err(|_| internal("package_recovery_receipt_utf8"))?,
    )
    .map_err(|_| internal("package_recovery_receipt_json"))?;
    let receipt: PackageReleaseReceipt =
        serde_json::from_value(value).map_err(|_| internal("package_recovery_receipt_shape"))?;
    receipt
        .validate_against_plan(plan)
        .map_err(|_| internal("package_recovery_receipt_plan_mismatch"))?;
    if receipt
        .canonical_bytes()
        .map_err(|_| internal("package_recovery_receipt_encode"))?
        != bytes
    {
        return Err(internal("package_recovery_receipt_not_canonical"));
    }
    Ok(receipt)
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest.as_ref() {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
