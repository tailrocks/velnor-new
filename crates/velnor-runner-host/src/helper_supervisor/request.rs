// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use base64::Engine;
use serde::Serialize;
use serde_json::Value;

use crate::error::HostError;

const MAX_BUNDLE_BYTES: usize = 2_000_000;
const MAX_REQUEST_BYTES: usize = 3 * 1024 * 1024;
const MAX_STATE_DIRECTORY_BYTES: usize = 4096;
const REQUEST_SCHEMA: u8 = 3;

#[derive(Serialize)]
struct Request<'a> {
    schema: u8,
    state_directory: &'a str,
    bundle_base64: String,
    expected: Expected<'a>,
    subject: SubjectRef<'a>,
    peer_subject: SubjectRef<'a>,
}

#[derive(Serialize)]
struct Expected<'a> {
    signer: &'a str,
    signer_digest: &'a str,
    source: &'a str,
    source_digest: &'a str,
    source_ref: &'a str,
    build_config: &'a str,
    build_config_digest: &'a str,
}

#[derive(Serialize, Clone, Copy)]
pub(crate) struct SubjectRef<'a> {
    pub(crate) name: &'a str,
    pub(crate) digest: &'a str,
}

pub(crate) struct SubjectCheck<'a> {
    pub(crate) state_directory: &'a Path,
    pub(crate) bundle: &'a Value,
    pub(crate) source_sha: &'a str,
    pub(crate) authority_sha: &'a str,
    pub(crate) subject: SubjectRef<'a>,
    pub(crate) peer_subject: SubjectRef<'a>,
}

pub(super) fn encode(target: &SubjectCheck<'_>) -> Result<Vec<u8>, HostError> {
    if target.subject.name.is_empty() || target.peer_subject.name.is_empty() {
        return Err(HostError::Identity);
    }
    let state_directory = target.state_directory.to_str().ok_or(HostError::Path)?;
    if !valid_state_directory(state_directory) {
        return Err(HostError::Path);
    }
    let bundle_bytes = serde_json::to_vec(target.bundle).map_err(|_| HostError::Identity)?;
    if bundle_bytes.is_empty() || bundle_bytes.len() > MAX_BUNDLE_BYTES {
        return Err(HostError::Frame);
    }
    let expected = Expected {
        signer: "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main",
        signer_digest: target.authority_sha,
        source: "https://github.com/tailrocks/velnor-new",
        source_digest: target.source_sha,
        source_ref: "refs/heads/main",
        build_config: "https://github.com/tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main",
        build_config_digest: target.authority_sha,
    };
    let request = Request {
        schema: REQUEST_SCHEMA,
        state_directory,
        bundle_base64: base64::engine::general_purpose::STANDARD.encode(bundle_bytes),
        expected,
        subject: target.subject,
        peer_subject: target.peer_subject,
    };
    let bytes = serde_json::to_vec(&request).map_err(|_| HostError::Identity)?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(HostError::Frame);
    }
    Ok(bytes)
}

fn valid_state_directory(value: &str) -> bool {
    let Some(components) = value.strip_prefix('/') else {
        return false;
    };
    !components.is_empty()
        && value.len() <= MAX_STATE_DIRECTORY_BYTES
        && value.chars().all(|ch| !ch.is_control())
        && components
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}
