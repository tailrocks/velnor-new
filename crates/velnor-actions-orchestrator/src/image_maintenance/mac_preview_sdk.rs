// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use std::path::Path;
use velnor_actions_contract::{
    GeneratorLock,
    runner_image_manifest::mac_attestation::{MacSdkAuthority, MacSdkIdentity},
    source_preview::PUBLICATION_AUTHORITY_SHA,
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedPreview {
    source_sha: String,
    version: String,
    receipt_sha256: String,
    bundle_sha256: String,
    release_id: u64,
    authority_sha: String,
    linux_sha256: String,
    darwin_sha256: String,
}

pub(super) struct VerifyRequest<'a> {
    pub source: &'a str,
    pub run: u64,
    pub attempt: u32,
    pub lock: &'a GeneratorLock,
    pub gh: &'a Path,
    pub temp: &'a Path,
    pub token: &'a str,
    pub deadline: CheckDeadline,
}

pub(super) fn verify(request: &VerifyRequest<'_>) -> Result<MacSdkIdentity, OrchestratorError> {
    let invalid = || internal("mac_preview_sdk_verification_failed");
    let preview = request.lock.runtime_preview.as_ref().ok_or_else(invalid)?;
    preview.validate(request.source).map_err(|_| invalid())?;
    let timeout = request.deadline.remaining().map_err(|_| invalid())?;
    let command = velnor_actions_mise::source_preview::verify_only_command(
        request.source,
        request.run,
        request.attempt,
        request.gh,
        request.temp,
        request.token,
    )
    .map_err(|_| invalid())?;
    let result = command.run_bounded(65536, timeout).map_err(|_| invalid())?;
    if !result.success {
        return Err(invalid());
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&result.stdout, 65536)
            .map_err(|_| invalid())?;
    let verified: VerifiedPreview = serde_json::from_value(value).map_err(|_| invalid())?;
    if verified.source_sha != preview.receipt.source_sha
        || verified.version != preview.receipt.version
        || verified.receipt_sha256 != preview.receipt_sha256
        || verified.bundle_sha256 != preview.bundle_sha256
        || verified.release_id != preview.receipt.release_id
        || verified.authority_sha != PUBLICATION_AUTHORITY_SHA
        || verified.linux_sha256 != preview.receipt.targets.linux.payload.sha256
        || verified.darwin_sha256 != preview.receipt.targets.darwin.payload.sha256
        || crate::homebrew::sdk_binding::sha256_current_executable(request.deadline, 268_435_456)?
            != verified.linux_sha256
    {
        return Err(invalid());
    }
    Ok(MacSdkIdentity {
        repository: "tailrocks/velnor-new".to_owned(),
        version: verified.version,
        source_commit: verified.source_sha,
        target: "aarch64-apple-darwin".to_owned(),
        asset_sha256: verified.darwin_sha256,
        authority: MacSdkAuthority::SourcePreview {
            receipt_sha256: verified.receipt_sha256,
            bundle_sha256: verified.bundle_sha256,
            authority_sha: verified.authority_sha,
            release_id: verified.release_id,
        },
    })
}
