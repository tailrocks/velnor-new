// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::images::IMAGE_REGISTRY_HOST;
use super::workflow_task::is_valid_workflow_task_id;
use crate::errors::ContractError;

const MAX_IMAGE_ID_BYTES: usize = 40;

#[must_use]
pub fn is_valid_image_id(id: &str) -> bool {
    id.len() <= MAX_IMAGE_ID_BYTES && is_valid_workflow_task_id(id)
}

#[must_use]
pub fn is_valid_image_repository(repository: &str) -> bool {
    let Some(rest) = repository
        .strip_prefix(IMAGE_REGISTRY_HOST)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    let segments = rest.split('/').collect::<Vec<_>>();
    segments.len() >= 2
        && segments.iter().enumerate().all(|(position, segment)| {
            if position == 0 {
                is_owner_segment(segment)
            } else {
                is_name_segment(segment)
            }
        })
}

fn is_owner_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('-')
        && !segment.ends_with('-')
        && segment
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_name_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && !segment.contains("..")
        && bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
}

pub(super) fn is_cargo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && !name.starts_with('-')
}

pub(super) fn require_safe_path(
    file: &str,
    key: String,
    path: &str,
    allow_dot: bool,
) -> Result<(), ContractError> {
    if allow_dot && path == "." {
        return Ok(());
    }
    let bad = path.is_empty()
        || path.len() > 256
        || path.starts_with('/')
        || path.ends_with('/')
        || path.split('/').any(|c| matches!(c, "" | "." | ".."))
        || !path.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b'+' | b'@')
        });
    if bad {
        return Err(ContractError::config(
            file,
            key,
            format!("bad_image_path:{path}"),
        ));
    }
    Ok(())
}
