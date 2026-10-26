// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{check_deadline, internal};
use crate::OrchestratorError;

pub(super) fn verify_digest(
    path: &Path,
    expected_size: u64,
    expected_digest: &str,
    maximum: u64,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("github_evidence_file_missing"))?;
    check_deadline(deadline)?;
    if !metadata.file_type().is_file()
        || expected_size == 0
        || expected_size > maximum
        || metadata.len() != expected_size
    {
        return Err(internal("github_evidence_file_size"));
    }
    let mut file = File::open(path).map_err(|_| internal("github_evidence_file_read"))?;
    check_deadline(deadline)?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        check_deadline(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_file_read"))?;
        if count == 0 {
            break;
        }
        let count_bytes =
            u64::try_from(count).map_err(|_| internal("github_evidence_file_size"))?;
        size = size
            .checked_add(count_bytes)
            .filter(|size| *size <= maximum)
            .ok_or_else(|| internal("github_evidence_file_size"))?;
        hash.update(&buffer[..count]);
    }
    check_deadline(deadline)?;
    if size != expected_size || hex_lower(&hash.finalize()) != expected_digest {
        return Err(internal("github_evidence_file_digest"));
    }
    Ok(())
}

pub(super) fn read_bounded(
    path: &Path,
    limit: u64,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    check_deadline(deadline)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("github_evidence_file_missing"))?;
    check_deadline(deadline)?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(internal("github_evidence_file_size"));
    }
    let mut file = File::open(path).map_err(|_| internal("github_evidence_file_read"))?;
    check_deadline(deadline)?;
    let capacity =
        usize::try_from(metadata.len()).map_err(|_| internal("github_evidence_file_size"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        check_deadline(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_file_read"))?;
        if count == 0 {
            break;
        }
        let next_size = bytes
            .len()
            .checked_add(count)
            .ok_or_else(|| internal("github_evidence_file_size"))?;
        if u64::try_from(next_size).map_or(true, |size| size > limit) {
            return Err(internal("github_evidence_file_size"));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    check_deadline(deadline)?;
    if u64::try_from(bytes.len()).ok() != Some(metadata.len())
        || u64::try_from(bytes.len()).map_or(true, |size| size > limit)
    {
        return Err(internal("github_evidence_file_size"));
    }
    Ok(bytes)
}

pub(super) fn copy_bounded(
    source: &Path,
    target: &Path,
    expected_size: u64,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let metadata =
        fs::symlink_metadata(source).map_err(|_| internal("github_evidence_file_missing"))?;
    check_deadline(deadline)?;
    if !metadata.file_type().is_file() || metadata.len() != expected_size {
        return Err(internal("github_evidence_file_size"));
    }
    let mut input = File::open(source).map_err(|_| internal("github_evidence_file_read"))?;
    check_deadline(deadline)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| internal("github_evidence_stage_write"))?;
    check_deadline(deadline)?;
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        check_deadline(deadline)?;
        let count = input
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_file_read"))?;
        if count == 0 {
            break;
        }
        let count_bytes =
            u64::try_from(count).map_err(|_| internal("github_evidence_file_size"))?;
        copied = copied
            .checked_add(count_bytes)
            .filter(|size| *size <= expected_size)
            .ok_or_else(|| internal("github_evidence_file_size"))?;
        output
            .write_all(&buffer[..count])
            .map_err(|_| internal("github_evidence_stage_write"))?;
    }
    if copied != expected_size {
        return Err(internal("github_evidence_file_size"));
    }
    check_deadline(deadline)?;
    output
        .sync_all()
        .map_err(|_| internal("github_evidence_stage_write"))?;
    check_deadline(deadline)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    result
}
