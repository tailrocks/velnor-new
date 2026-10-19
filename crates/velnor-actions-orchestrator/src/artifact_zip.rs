//! Closed, bounded ZIP admission shared by native leaf and signed recovery readers.
//!
//! This module knows file identities, never a leaf/receipt schema or signature.
//! Optional expected hashes belong only to the historical signed-recovery caller;
//! unsigned leaf callers must supply their complete exact size/hash/mode inventory.
use crate::{OrchestratorError, internal::internal};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
};
use velnor_actions_mise::CheckDeadline;

#[path = "artifact_zip_output.rs"]
mod output;
#[path = "artifact_zip_reader.rs"]
mod reader;
use reader::Archive;
const MAX_ENTRIES: usize = 4096;
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// One exact flat file, with optional historical signature-bundle byte identity.
pub(crate) struct ExpectedZipEntry {
    pub name: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub max_bytes: u64,
    pub unix_permissions: Option<u32>,
}

/// Read one fixed manifest only after validating the complete archive headers.
pub(crate) fn read_manifest(
    bytes: &[u8],
    name: &str,
    max_bytes: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    deadline
        .scope(|| read_manifest_in_scope(bytes, name, max_bytes))
        .map_err(|_| invalid("deadline"))?
}

/// Extract an exact inventory into an existing empty directory owned by the caller.
/// The directory is never removed. Failed work removes only pinned files created here.
pub(crate) fn extract(
    bytes: &[u8],
    destination: &Path,
    expected: &[ExpectedZipEntry],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check()?;
    let mut output = output::Output::open(destination)?;
    let result = deadline
        .scope(|| extract_into(bytes, &mut output, expected))
        .map_err(|_| invalid("deadline"))
        .and_then(|result| result);
    finish(result, &mut output)
}

pub(super) fn read_manifest_in_scope(
    bytes: &[u8],
    name: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    check()?;
    if !safe_name(name) || max_bytes == 0 || max_bytes > MAX_UNPACKED_BYTES {
        return Err(invalid("manifest_bound"));
    }
    let mut archive = reader::open(bytes)?;
    let mut file = archive
        .by_name(name)
        .map_err(|_| invalid("manifest_missing"))?;
    if file.name_raw() != name.as_bytes() || file.size() == 0 || file.size() > max_bytes {
        return Err(invalid("manifest_size"));
    }
    let mut output = Vec::new();
    let expected_size = file.size();
    let (copied, _) = copy(&mut file, &mut output, max_bytes)?;
    if copied != expected_size {
        return Err(invalid("manifest_size"));
    }
    check()?;
    Ok(output)
}

pub(super) fn extract_in_scope(
    bytes: &[u8],
    destination: &Path,
    expected: &[ExpectedZipEntry],
) -> Result<(), OrchestratorError> {
    check()?;
    let mut output = output::Output::open(destination)?;
    let result = extract_into(bytes, &mut output, expected);
    finish(result, &mut output)
}

fn finish(
    result: Result<(), OrchestratorError>,
    output: &mut output::Output,
) -> Result<(), OrchestratorError> {
    match result {
        Ok(()) => {
            output.commit();
            Ok(())
        }
        Err(error) => {
            output.rollback()?;
            Err(error)
        }
    }
}

fn extract_into(
    bytes: &[u8],
    output: &mut output::Output,
    expected: &[ExpectedZipEntry],
) -> Result<(), OrchestratorError> {
    let mut archive = reader::open(bytes)?;
    let entries = admit(&mut archive, expected)?;
    for (index, expected) in entries {
        check()?;
        let mut file = archive.by_index(index).map_err(|_| invalid("entry"))?;
        let size = file.size();
        let mut out = output.create(&expected.name)?;
        let (copied, digest) = copy(&mut file, &mut out, size)?;
        if copied != size || expected.sha256.as_ref().is_some_and(|hash| hash != &digest) {
            return Err(invalid("file_identity"));
        }
        out.sync_all().map_err(|_| invalid("output_sync"))?;
        check()?;
    }
    output.validate()?;
    check()
}

fn admit<'a>(
    archive: &mut Archive<'_>,
    expected: &'a [ExpectedZipEntry],
) -> Result<Vec<(usize, &'a ExpectedZipEntry)>, OrchestratorError> {
    if expected.is_empty() || expected.len() > MAX_ENTRIES || expected.len() != archive.len() {
        return Err(invalid("inventory"));
    }
    let mut by_name = BTreeMap::new();
    for entry in expected {
        check()?;
        if !safe_name(&entry.name)
            || by_name.insert(entry.name.as_str(), entry).is_some()
            || entry.max_bytes == 0
            || entry.max_bytes > MAX_UNPACKED_BYTES
            || entry.size.is_some_and(|n| n > entry.max_bytes)
            || entry.unix_permissions.is_some_and(|mode| mode > 0o7777)
            || entry.sha256.as_ref().is_some_and(|h| {
                h.len() != 64
                    || !h
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(invalid("expectation"));
        }
    }
    let mut admitted = Vec::with_capacity(expected.len());
    let mut observed = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        check()?;
        let file = archive.by_index(index).map_err(|_| invalid("entry"))?;
        let name = file.name().map_err(|_| invalid("path"))?;
        let expected = by_name
            .get(name.as_ref())
            .ok_or_else(|| invalid("unexpected_entry"))?;
        if !observed.insert(name.into_owned())
            || file.name_raw() != expected.name.as_bytes()
            || file.size() > expected.max_bytes
            || expected.size.is_some_and(|n| n != file.size())
            || expected.unix_permissions.is_some_and(|mode| {
                file.unix_mode()
                    .is_none_or(|actual| actual & 0o170_000 != 0o100_000 || actual & 0o7777 != mode)
            })
        {
            return Err(invalid("file_type_size_mode"));
        }
        total = total
            .checked_add(file.size())
            .filter(|n| *n <= MAX_UNPACKED_BYTES)
            .ok_or_else(|| invalid("unpacked_limit"))?;
        admitted.push((index, *expected));
    }
    Ok(admitted)
}

/// Stream through EOF: the ZIP reader checks CRC; our counter/hash bind actual output.
fn copy(
    input: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
) -> Result<(u64, String), OrchestratorError> {
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        check()?;
        let count = input
            .read(&mut buffer)
            .map_err(|_| invalid("read_or_crc"))?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .filter(|n| *n <= limit)
            .ok_or_else(|| invalid("unpacked_limit"))?;
        output
            .write_all(&buffer[..count])
            .map_err(|_| invalid("write"))?;
        hasher.update(&buffer[..count]);
        check()?;
    }
    Ok((copied, hex_digest(hasher.finalize())))
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
fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0', ':'])
}
fn check() -> Result<(), OrchestratorError> {
    CheckDeadline::check_scope().map_err(|_| invalid("deadline"))
}
fn invalid(reason: &str) -> OrchestratorError {
    internal(&format!("artifact_zip_{reason}"))
}

/// One native recovery/promotion budget, intersected with any enclosing operation.
pub(super) fn within_operation<T>(
    operation: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let budget = std::time::Duration::from_secs(
        u64::from(velnor_actions_contract::workflow::JobTimeout::MAX_MINUTES) * 60,
    );
    let deadline =
        CheckDeadline::from_start_with_cleanup_reserve(std::time::Instant::now(), budget)
            .map_err(|_| invalid("deadline"))?;
    deadline.scope(operation).map_err(|_| invalid("deadline"))?
}

#[cfg(test)]
#[path = "artifact_zip_tests.rs"]
mod tests;
