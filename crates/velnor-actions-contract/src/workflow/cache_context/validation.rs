// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::canonical::normalize_posix_path;
use crate::errors::ContractError;
use crate::ids::is_lower_hex_len;

pub(super) fn require_git_sha(field: &'static str, value: &str) -> Result<(), ContractError> {
    if !is_lower_hex_len(value, 40) {
        return Err(ContractError::identity(field, "malformed_commit"));
    }
    Ok(())
}

pub(super) fn require_absolute_path(field: &'static str, value: &str) -> Result<(), ContractError> {
    let stripped = value
        .strip_prefix('/')
        .ok_or_else(|| ContractError::identity(field, "not_absolute_path"))?;
    if stripped.is_empty()
        || stripped
            .split('/')
            .any(|segment| segment.is_empty() || segment == "..")
        || stripped.bytes().any(|b| b < 0x20 || b == 0x7f)
    {
        return Err(ContractError::identity(field, "invalid_path"));
    }
    Ok(())
}

pub(super) fn require_relative_path(field: &'static str, value: &str) -> Result<(), ContractError> {
    normalize_posix_path(value).map_err(|_| ContractError::identity(field, "invalid_path"))?;
    Ok(())
}

pub(super) fn validate_rfc3339_utc(field: &'static str, value: &str) -> Result<(), ContractError> {
    let invalid = || ContractError::identity(field, "malformed_timestamp");
    let (date, time) = value.split_once('T').ok_or_else(invalid)?;
    let date_parts: Vec<&str> = date.split('-').collect();
    if date_parts.len() != 3
        || date_parts[0].len() != 4
        || date_parts[1].len() != 2
        || date_parts[2].len() != 2
        || !date_parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let time = time.strip_suffix('Z').ok_or_else(invalid)?;
    let (clock, _) = match time.split_once('.') {
        Some((clock, fraction)) => {
            if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            (clock, true)
        }
        None => (time, false),
    };
    let clock_parts: Vec<&str> = clock.split(':').collect();
    if clock_parts.len() != 3
        || clock_parts.iter().any(|part| part.len() != 2)
        || !clock_parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    Ok(())
}
