// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;

pub(crate) fn is_clean_text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && !value.contains("${{")
        && !value.chars().any(char::is_control)
}

pub fn validate_environment(name: &str) -> Result<(), RenderError> {
    let charset = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.');
    if is_clean_text(name, 255)
        && name.bytes().all(charset)
        && name
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
    {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_environment:{name}"
        )))
    }
}

fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub fn validate_repository(repository: &str) -> Result<(), RenderError> {
    let invalid = RenderError::InvalidWorkflow(format!("bad_repository:{repository}"));
    if !is_clean_text(repository, 201) || repository.contains(char::is_whitespace) {
        return Err(invalid);
    }
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(invalid);
    };
    if name.contains('/') || !is_repo_segment(owner) || !is_repo_segment(name) {
        return Err(invalid);
    }
    Ok(())
}

pub fn validate_source_sha(sha: &str) -> Result<(), RenderError> {
    let hex = |b: u8| b.is_ascii_digit() || matches!(b, b'a'..=b'f');
    if sha.len() == 40 && sha.bytes().all(hex) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_source_sha:{sha}"
        )))
    }
}

pub fn validate_plan_id(id: &str) -> Result<(), RenderError> {
    let charset = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-');
    if is_clean_text(id, 128) && id.bytes().all(charset) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!("bad_plan_id:{id}")))
    }
}

pub fn validate_package_name(name: &str) -> Result<(), RenderError> {
    validate_release_identifier(name, false)
}

pub(crate) fn validate_version_group(name: &str) -> Result<(), RenderError> {
    validate_release_identifier(name, true)
}

fn validate_release_identifier(name: &str, allow_long: bool) -> Result<(), RenderError> {
    let mut bytes = name.bytes();
    let leading = bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    let rest = bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    if leading && rest && (allow_long || name.len() <= 64) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_package_name:{name}"
        )))
    }
}

pub fn validate_package_version(version: &str) -> Result<(), RenderError> {
    if version.len() <= 150 && semver::Version::parse(version).is_ok() {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_package_version:{version}"
        )))
    }
}
