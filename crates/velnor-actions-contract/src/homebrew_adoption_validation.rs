// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{
    ContractError, HomebrewAdoptionFileIdentity, HomebrewAdoptionTreeChange,
    HomebrewAdoptionTreeFile,
};

pub(super) fn validate_identities(
    identities: &[HomebrewAdoptionFileIdentity],
    code: &'static str,
) -> Result<(), ContractError> {
    if identities.is_empty()
        || !identities
            .windows(2)
            .all(|items| items[0].path < items[1].path)
        || identities.iter().any(|item| {
            !safe_repo_path(&item.path)
                || !matches!(item.mode, 0o100_644 | 0o100_755)
                || !full_sha(&item.blob_sha)
        })
    {
        return Err(invalid(code));
    }
    Ok(())
}

pub(super) fn tree_map(
    files: &[HomebrewAdoptionTreeFile],
) -> Result<BTreeMap<String, HomebrewAdoptionTreeFile>, ContractError> {
    let mut map = BTreeMap::new();
    for file in files {
        if !safe_repo_path(&file.path)
            || !matches!(file.mode, 0o100_644 | 0o100_755 | 0o120_000)
            || !full_sha(&file.blob_sha)
            || map.insert(file.path.clone(), file.clone()).is_some()
        {
            return Err(invalid("homebrew_adoption_tree_invalid"));
        }
    }
    Ok(map)
}

pub(super) fn change_map(
    changes: &[HomebrewAdoptionTreeChange],
) -> Result<BTreeMap<String, HomebrewAdoptionTreeChange>, ContractError> {
    let mut map = BTreeMap::new();
    for change in changes {
        if !safe_repo_path(&change.path)
            || !matches!(change.status.as_str(), "A" | "D" | "M" | "T")
            || !valid_diff_shape(change)
            || change.old_mode.is_some_and(|mode| !valid_old_mode(mode))
            || change.new_mode.is_some_and(|mode| !valid_new_mode(mode))
            || change
                .old_blob_sha
                .as_deref()
                .is_some_and(|sha| !full_sha(sha))
            || change
                .new_blob_sha
                .as_deref()
                .is_some_and(|sha| !full_sha(sha))
            || map.insert(change.path.clone(), change.clone()).is_some()
        {
            return Err(invalid("homebrew_adoption_diff_invalid"));
        }
    }
    Ok(map)
}

fn valid_diff_shape(change: &HomebrewAdoptionTreeChange) -> bool {
    match change.status.as_str() {
        "A" => {
            change.old_mode.is_none()
                && change.old_blob_sha.is_none()
                && change.new_mode.is_some()
                && change.new_blob_sha.is_some()
        }
        "D" => {
            change.old_mode.is_some()
                && change.old_blob_sha.is_some()
                && change.new_mode.is_none()
                && change.new_blob_sha.is_none()
        }
        "M" | "T" => {
            change.old_mode.is_some()
                && change.old_blob_sha.is_some()
                && change.new_mode.is_some()
                && change.new_blob_sha.is_some()
        }
        _ => false,
    }
}

fn valid_old_mode(mode: u32) -> bool {
    matches!(mode, 0o100_644 | 0o100_755 | 0o120_000)
}

fn valid_new_mode(mode: u32) -> bool {
    matches!(mode, 0o100_644 | 0o100_755)
}

pub(super) fn safe_transition_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(super) fn stable_semver(value: &str) -> bool {
    let mut components = value.split('.');
    let Some(major) = components.next() else {
        return false;
    };
    let Some(minor) = components.next() else {
        return false;
    };
    let Some(patch) = components.next() else {
        return false;
    };
    components.next().is_none()
        && [major, minor, patch].iter().all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && (component == &"0" || !component.starts_with('0'))
        })
}

pub(super) fn safe_repo_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn strict_sorted_unique<T: Ord>(items: &[T]) -> bool {
    items.windows(2).all(|pair| pair[0] < pair[1])
}

pub(super) fn full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn invalid(code: &'static str) -> ContractError {
    ContractError::CanonicalJson(code.to_owned())
}

pub(super) fn invalid_owned(code: String) -> ContractError {
    ContractError::CanonicalJson(code)
}
