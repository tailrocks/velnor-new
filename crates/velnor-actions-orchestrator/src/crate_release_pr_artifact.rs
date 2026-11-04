// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationBinding {
    pub repository: String,
    pub workflow_sha: String,
    pub source_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedFile {
    pub path: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationArtifact {
    pub schema: u32,
    pub binding: PreparationBinding,
    pub files: Vec<PreparedFile>,
}

pub fn verify_preparation_artifact(
    bytes: &[u8],
    digest: &str,
    binding: &PreparationBinding,
    allowed_paths: &BTreeSet<String>,
) -> Result<PreparationArtifact, &'static str> {
    let invalid = || "release_pr_artifact_invalid";
    if bytes.len() > MAX_ARTIFACT_BYTES || digest != sha256(bytes) || !valid_binding(binding) {
        return Err(invalid());
    }
    let artifact: PreparationArtifact = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if artifact.schema != 1 || artifact.binding != *binding || artifact.files.len() > MAX_FILES {
        return Err(invalid());
    }
    let mut previous: Option<&str> = None;
    let mut total = 0usize;
    for file in &artifact.files {
        if !valid_path(&file.path)
            || !allowed_paths.contains(&file.path)
            || previous.is_some_and(|prior| prior >= file.path.as_str())
            || file.bytes.len() > MAX_FILE_BYTES
            || file.sha256 != sha256(&file.bytes)
        {
            return Err(invalid());
        }
        total = total.checked_add(file.bytes.len()).ok_or_else(invalid)?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid());
        }
        previous = Some(&file.path);
    }
    Ok(artifact)
}

fn valid_binding(binding: &PreparationBinding) -> bool {
    let Some((owner, repository)) = binding.repository.split_once('/') else {
        return false;
    };
    velnor_actions_contract::config::is_valid_github_owner(owner)
        && velnor_actions_contract::config::is_valid_github_repo_name(repository)
        && valid_git_sha(&binding.workflow_sha)
        && valid_git_sha(&binding.source_sha)
        && binding.run_id > 0
        && binding.run_attempt > 0
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.bytes().any(|byte| byte.is_ascii_control())
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}
