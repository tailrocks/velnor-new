// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use base64::Engine;
use serde::Serialize;
use serde_json::Value;

use crate::error::HostError;

const MAX_BUNDLE_BYTES: usize = 2_000_000;
const MAX_CHECKSUM_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BYTES: usize = 3 * 1024 * 1024;
const MAX_STATE_DIRECTORY_BYTES: usize = 4096;
const REQUEST_SCHEMA: u8 = 2;

#[derive(Serialize)]
struct Request<'a> {
    schema: u8,
    state_directory: &'a str,
    bundle_base64: String,
    checksum_base64: String,
    expected: Expected<'a>,
    checksum_subject: Subject<'a>,
    target_subject: Subject<'a>,
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

#[derive(Serialize)]
struct Subject<'a> {
    name: &'a str,
    digest: &'a str,
}

pub(crate) struct ChecksumTarget<'a> {
    pub(crate) state_directory: &'a Path,
    pub(crate) bundle: &'a Value,
    pub(crate) checksum_bytes: &'a [u8],
    pub(crate) source_sha: &'a str,
    pub(crate) authority_sha: &'a str,
    pub(crate) checksum_digest: &'a str,
    pub(crate) target_name: &'a str,
    pub(crate) target_digest: &'a str,
}

pub(super) fn encode(target: &ChecksumTarget<'_>) -> Result<Vec<u8>, HostError> {
    if target.checksum_bytes.is_empty()
        || target.checksum_bytes.len() > MAX_CHECKSUM_BYTES
        || target.target_name.is_empty()
    {
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
        build_config: "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main",
        build_config_digest: target.authority_sha,
    };
    let request = Request {
        schema: REQUEST_SCHEMA,
        state_directory,
        bundle_base64: base64::engine::general_purpose::STANDARD.encode(bundle_bytes),
        checksum_base64: base64::engine::general_purpose::STANDARD.encode(target.checksum_bytes),
        expected,
        checksum_subject: Subject {
            name: "SHA256SUMS",
            digest: target.checksum_digest,
        },
        target_subject: Subject {
            name: target.target_name,
            digest: target.target_digest,
        },
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
