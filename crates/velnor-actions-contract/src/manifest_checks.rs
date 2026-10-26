// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;

pub(crate) use crate::ids::is_lower_hex;

pub(crate) fn check_schema(schema: u32) -> Result<(), ContractError> {
    if schema != 1 {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}

pub(crate) fn check_semver(version: &str, file: &str, key: &str) -> Result<(), ContractError> {
    let parts: Vec<&str> = version.split('.').collect();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_semver"))
    }
}

pub(crate) fn check_target(target: &str, file: &str, key: &str) -> Result<(), ContractError> {
    let valid = !target.is_empty()
        && target.contains('-')
        && target.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_target"))
    }
}

pub(crate) fn check_immutable_url(url: &str, file: &str, key: &str) -> Result<(), ContractError> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(ContractError::config(file, key, "mutable_or_malformed_url"));
    };
    let valid = !rest.is_empty()
        && !url.contains(' ')
        && !rest.split('/').any(|seg| seg.is_empty() || seg == "latest");
    if valid {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "mutable_or_malformed_url"))
    }
}

pub(crate) fn check_sha256(sha: &str, file: &str, key: &str) -> Result<(), ContractError> {
    if crate::ids::is_lower_hex_len(sha, 64) {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_sha256"))
    }
}

pub(crate) fn check_commit(commit: &str, file: &str, key: &str) -> Result<(), ContractError> {
    if crate::ids::is_lower_hex_len(commit, 40) {
        Ok(())
    } else {
        Err(ContractError::config(file, key, "malformed_commit"))
    }
}

pub(crate) fn is_review_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || !parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    let month: u32 = parts[1].parse().unwrap_or(0);
    let day: u32 = parts[2].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}
