// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::super::strict_json;
use super::{EXPECTED_TARGETS, MAX_MANIFEST_BYTES};
use crate::owned_archive_guard::qualification::filesystem::read_regular;

#[derive(Clone, Debug)]
pub(super) struct TargetBinding {
    pub(super) target: String,
    pub(super) digest: String,
}

#[derive(Clone, Debug)]
pub(super) struct CandidateManifest {
    pub(super) commit: String,
    pub(super) by_url: BTreeMap<String, TargetBinding>,
}

pub(super) fn read(path: &Path) -> Result<CandidateManifest, String> {
    let bytes = read_regular(path, MAX_MANIFEST_BYTES)?;
    let value = strict_json::parse(&bytes)
        .map_err(|error| format!("malformed or duplicate-key JSON: {error}"))?;
    validate(&value)
}

fn validate(value: &Value) -> Result<CandidateManifest, String> {
    if value.get("schema").and_then(Value::as_u64) != Some(1)
        || value.get("repository").and_then(Value::as_str) != Some("tailrocks/velnor-new")
    {
        return Err("candidate manifest does not match the release schema or repository".into());
    }
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| is_release_version(value))
        .ok_or_else(|| "candidate manifest version is malformed".to_owned())?;
    let commit = value
        .get("commit")
        .and_then(Value::as_str)
        .filter(|value| is_lower_hex(value, 40))
        .ok_or_else(|| "candidate manifest source commit is malformed".to_owned())?
        .to_owned();
    let targets = value
        .get("targets")
        .and_then(Value::as_array)
        .filter(|targets| targets.len() == EXPECTED_TARGETS.len())
        .ok_or_else(|| "candidate manifest target inventory is malformed".to_owned())?;
    let mut by_url = BTreeMap::new();
    for (item, expected) in targets.iter().zip(EXPECTED_TARGETS) {
        let target = item.get("target").and_then(Value::as_str);
        let digest = item.get("sha256").and_then(Value::as_str);
        let artifact = item.get("artifact").and_then(Value::as_str);
        let expected_url = format!(
            "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{expected}"
        );
        if target != Some(expected)
            || !digest.is_some_and(|value| is_lower_hex(value, 64))
            || artifact != Some(expected_url.as_str())
        {
            return Err(
                "candidate manifest does not match the release schema or target inventory".into(),
            );
        }
        let (Some(target), Some(digest), Some(artifact)) = (target, digest, artifact) else {
            return Err("candidate manifest target inventory is malformed".into());
        };
        let binding = TargetBinding {
            target: target.to_owned(),
            digest: digest.to_owned(),
        };
        if by_url.insert(artifact.to_owned(), binding).is_some() {
            return Err("candidate manifest contains duplicate release URLs".into());
        }
    }
    Ok(CandidateManifest { commit, by_url })
}

fn is_release_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
