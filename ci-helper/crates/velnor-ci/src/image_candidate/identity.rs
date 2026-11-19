// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::json::is_hex;
use crate::error::CiError;

fn fail<T>(message: impl Into<String>) -> Result<T, CiError> {
    Err(CiError::new(message))
}

fn name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

#[must_use]
pub fn is_repository(text: &str) -> bool {
    text.split_once('/').is_some_and(|(owner, name)| {
        [owner, name]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(name_char))
    })
}

#[must_use]
pub fn is_event(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

#[must_use]
pub fn is_workflow_ref(repository: &str, text: &str) -> bool {
    text.starts_with(&format!("{repository}/.github/workflows/")) && text.contains("@refs/")
}

#[must_use]
pub fn buildx_version(builder: &str) -> Option<&str> {
    let rest = builder.strip_prefix("github.com/docker/buildx v")?;
    let (version, tail) = match rest.split_once(' ') {
        Some((version, tail)) => (version, Some(tail)),
        None => (rest, None),
    };
    if tail.is_some_and(|tail| tail.is_empty() || tail.contains('\n')) {
        return None;
    }
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = |part: &&str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    (parts.len() == 3 && parts.iter().all(numeric)).then_some(version)
}

fn positive(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().filter(|number| *number >= 1)
}

pub fn workflow<S: std::hash::BuildHasher>(
    env: &HashMap<String, String, S>,
) -> Result<(Map<String, Value>, String), CiError> {
    let get = |name: &str| env.get(name).map_or("", String::as_str);
    let (repository, reference) = (get("GITHUB_REPOSITORY"), get("GITHUB_REF"));
    if !reference.starts_with("refs/") || reference.contains('\n') {
        return fail("GitHub source ref is missing or incorrect.");
    }
    if !is_repository(repository) {
        return fail("GitHub repository identity is incorrect.");
    }
    let Some(repository_id) = positive(get("GITHUB_REPOSITORY_ID")) else {
        return fail("GitHub repository ID is incorrect.");
    };
    if !is_event(get("GITHUB_EVENT_NAME")) {
        return fail("GitHub event name is incorrect.");
    }
    if !is_workflow_ref(repository, get("GITHUB_WORKFLOW_REF")) {
        return fail("GitHub workflow ref is incorrect.");
    }
    if !is_hex(get("GITHUB_WORKFLOW_SHA"), 40) {
        return fail("GitHub workflow authority commit is incorrect.");
    }
    let Some(run_id) = positive(get("GITHUB_RUN_ID")) else {
        return fail("GitHub run_id is incorrect.");
    };
    let Some(run_attempt) = positive(get("GITHUB_RUN_ATTEMPT")) else {
        return fail("GitHub run_attempt is incorrect.");
    };
    let Value::Object(fields) = json!({
        "repository": repository,
        "repository_id": repository_id,
        "event": get("GITHUB_EVENT_NAME"),
        "workflow_ref": get("GITHUB_WORKFLOW_REF"),
        "authority_commit": get("GITHUB_WORKFLOW_SHA"),
        "run_id": run_id,
        "run_attempt": run_attempt,
    }) else {
        return fail("GitHub workflow identity is incorrect.");
    };
    Ok((fields, reference.to_owned()))
}
