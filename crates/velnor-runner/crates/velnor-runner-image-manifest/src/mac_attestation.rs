// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ImageContractError,
    mac_qualification::{MacRunnerQualificationReceipt, parse_mac_runner_qualification},
    strict_json,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

pub const MAC_ATTESTATION_SCHEMA: &str = "velnor.runner-image-mac-attestation.v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacQualificationAttestation {
    pub schema: String,
    pub public_authority: MacPublicAuthority,
    pub companion: MacCompanionAuthority,
    pub sdk: MacSdkIdentity,
    pub qualification: MacQualificationEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacPublicAuthority {
    pub repository: String,
    pub repository_id: u64,
    pub source_commit: String,
    pub release_run_id: u64,
    pub release_run_attempt: u32,
    pub producer_artifact_id: u64,
    pub publisher_artifact_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacCompanionAuthority {
    pub repository: String,
    pub repository_id: u64,
    pub owner_id: u64,
    pub workflow_path: String,
    pub workflow_ref: String,
    pub workflow_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub qualification_job_id: u64,
    pub runner_id: u64,
    pub receipt_artifact_id: u64,
    pub receipt_artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacSdkIdentity {
    pub repository: String,
    pub version: String,
    pub source_commit: String,
    pub authority: MacSdkAuthority,
    pub target: String,
    pub asset_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "channel", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MacSdkAuthority {
    Stable {
        release_manifest_sha256: String,
    },
    SourcePreview {
        receipt_sha256: String,
        bundle_sha256: String,
        authority_sha: String,
        release_id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacQualificationEvidence {
    pub receipt_base64: String,
    pub native_receipt_sha256: String,
}

fn valid_sdk_authority(authority: &MacSdkAuthority) -> bool {
    match authority {
        MacSdkAuthority::Stable {
            release_manifest_sha256,
        } => hex(release_manifest_sha256, 64),
        MacSdkAuthority::SourcePreview {
            receipt_sha256,
            bundle_sha256,
            authority_sha,
            release_id,
        } => {
            hex(receipt_sha256, 64)
                && hex(bundle_sha256, 64)
                && hex(authority_sha, 40)
                && *release_id != 0
        }
    }
}

pub fn parse_mac_qualification_attestation(
    bytes: &[u8],
) -> Result<(MacQualificationAttestation, MacRunnerQualificationReceipt), ImageContractError> {
    let invalid =
        || ImageContractError::CanonicalJson("mac_qualification_attestation_invalid".to_owned());
    let value =
        strict_json::parse_strict_json_bytes(bytes, 2 * 1024 * 1024).map_err(|_| invalid())?;
    let envelope: MacQualificationAttestation =
        serde_json::from_value(value).map_err(|_| invalid())?;
    let public = &envelope.public_authority;
    let companion = &envelope.companion;
    let sdk = &envelope.sdk;
    if envelope.schema != MAC_ATTESTATION_SCHEMA
        || public.repository != "tailrocks/velnor-new"
        || public.repository_id != 1_390_620_900
        || !hex(&public.source_commit, 40)
        || public.release_run_id == 0
        || public.release_run_attempt == 0
        || public.producer_artifact_id == 0
        || public.publisher_artifact_id == 0
        || companion.repository != "donbeave/velnor-mac-qualification"
        || companion.repository_id != 1_450_917_587
        || companion.owner_id != 139_017
        || !workflow_path(&companion.workflow_path)
        || companion.workflow_ref != "refs/heads/main"
        || !hex(&companion.workflow_sha, 40)
        || companion.run_id == 0
        || companion.run_attempt == 0
        || companion.qualification_job_id == 0
        || companion.runner_id == 0
        || companion.receipt_artifact_id == 0
        || !digest(&companion.receipt_artifact_digest)
        || sdk.repository != "tailrocks/velnor-new"
        || !version(&sdk.version)
        || !hex(&sdk.source_commit, 40)
        || !valid_sdk_authority(&sdk.authority)
        || sdk.target != "aarch64-apple-darwin"
        || !hex(&sdk.asset_sha256, 64)
        || !hex(&envelope.qualification.native_receipt_sha256, 64)
    {
        return Err(invalid());
    }
    let receipt_bytes = STANDARD
        .decode(&envelope.qualification.receipt_base64)
        .map_err(|_| invalid())?;
    if STANDARD.encode(&receipt_bytes) != envelope.qualification.receipt_base64
        || receipt_bytes.len() > 1024 * 1024
        || hex_bytes(&Sha256::digest(&receipt_bytes))
            != envelope.qualification.native_receipt_sha256
    {
        return Err(invalid());
    }
    let receipt = parse_mac_runner_qualification(&receipt_bytes).map_err(|_| invalid())?;
    if receipt.source_commit != public.source_commit
        || receipt.run_id != public.release_run_id
        || receipt.run_attempt != public.release_run_attempt
        || receipt.publisher_artifact_id != public.publisher_artifact_id
    {
        return Err(invalid());
    }
    Ok((envelope, receipt))
}

fn workflow_path(value: &str) -> bool {
    value
        .strip_prefix(".github/workflows/")
        .is_some_and(|name| {
            matches!(name.rsplit_once('.'), Some((_, "yml" | "yaml")))
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
                && !name.starts_with('.')
        })
}

fn version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| hex(value, 64))
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn hex_bytes(bytes: &[u8]) -> String {
    {
        const ALPHABET: &[u8; 16] = b"0123456789abcdef";
        bytes
            .iter()
            .flat_map(|byte| {
                [
                    char::from(ALPHABET[usize::from(byte >> 4)]),
                    char::from(ALPHABET[usize::from(byte & 15)]),
                ]
            })
            .collect()
    }
}

#[must_use]
pub fn receipt_matches_manifest(
    receipt: &MacRunnerQualificationReceipt,
    manifest: &crate::RunnerImageManifest,
    manifest_bytes: &[u8],
) -> bool {
    receipt.manifest_sha256 == hex_bytes(&Sha256::digest(manifest_bytes))
        && receipt.runner_lock_sha256 == manifest.runner_lock_sha256
        && receipt.source_commit == manifest.source_commit
        && receipt.platform == manifest.platform
        && receipt.versions.runner == manifest.versions.runner
        && receipt.versions.docker == manifest.versions.docker
        && receipt.versions.buildx == manifest.versions.buildx
        && receipt.versions.compose == manifest.versions.compose
        && receipt.versions.node == manifest.versions.node
        && same_image(
            &receipt.images.runner,
            &manifest.images.runner,
            &receipt.outer_daemon.store,
        )
        && same_image(
            &receipt.images.dind,
            &manifest.images.dind,
            &receipt.outer_daemon.store,
        )
        && same_image(
            &receipt.images.resource_probe,
            &manifest.images.resource_probe,
            &receipt.outer_daemon.store,
        )
}

fn same_image(
    actual: &crate::mac_qualification::MacQualificationImage,
    expected: &crate::ImageEntry,
    store: &str,
) -> bool {
    actual.repository == expected.repository
        && actual.registry_digest == expected.registry_digest
        && actual.platform_manifest_digest == expected.platform_manifest_digest
        && actual.config_digest == expected.config_digest
        && match store {
            "classic" => actual.local_id == expected.config_digest,
            "containerd" => actual.local_id == expected.registry_digest,
            _ => false,
        }
}
