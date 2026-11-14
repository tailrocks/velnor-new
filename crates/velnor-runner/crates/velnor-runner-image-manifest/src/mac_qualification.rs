// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ImageContractError;

pub const MAC_QUALIFICATION_SCHEMA: &str = "velnor.runner-image-mac-qualification.v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacRunnerQualificationReceipt {
    pub schema: String,
    pub source_commit: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub publisher_artifact_id: u64,
    pub manifest_sha256: String,
    pub runner_lock_sha256: String,
    pub platform: String,
    pub images: MacQualificationImages,
    pub outer_daemon: MacDaemonIdentity,
    pub inner_daemon_id: String,
    pub versions: MacQualificationVersions,
    pub cleanup_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacQualificationImages {
    pub runner: MacQualificationImage,
    pub dind: MacQualificationImage,
    pub resource_probe: MacQualificationImage,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacQualificationImage {
    pub repository: String,
    pub registry_digest: String,
    pub platform_manifest_digest: String,
    pub config_digest: String,
    pub local_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacDaemonIdentity {
    pub id: String,
    pub root: String,
    pub store: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacQualificationVersions {
    pub runner: String,
    pub docker: String,
    pub buildx: String,
    pub compose: String,
    pub node: String,
}

pub fn parse_mac_runner_qualification(
    bytes: &[u8],
) -> Result<MacRunnerQualificationReceipt, ImageContractError> {
    let invalid =
        || ImageContractError::CanonicalJson("mac_runner_qualification_invalid".to_owned());
    let value =
        crate::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024).map_err(|_| invalid())?;
    let receipt: MacRunnerQualificationReceipt =
        serde_json::from_value(value).map_err(|_| invalid())?;
    if receipt.schema != MAC_QUALIFICATION_SCHEMA
        || !hex(&receipt.source_commit, 40)
        || !hex(&receipt.manifest_sha256, 64)
        || !hex(&receipt.runner_lock_sha256, 64)
        || receipt.run_id == 0
        || receipt.run_attempt == 0
        || receipt.publisher_artifact_id == 0
        || receipt.platform != "linux/amd64"
        || !receipt.cleanup_complete
        || !bounded_text(&receipt.outer_daemon.id)
        || !bounded_text(&receipt.inner_daemon_id)
        || receipt.outer_daemon.id == receipt.inner_daemon_id
        || !receipt.outer_daemon.root.starts_with('/')
        || !bounded_text(&receipt.outer_daemon.root)
        || !bounded_text(&receipt.outer_daemon.store)
        || [
            &receipt.versions.runner,
            &receipt.versions.docker,
            &receipt.versions.buildx,
            &receipt.versions.compose,
            &receipt.versions.node,
        ]
        .iter()
        .any(|v| v.is_empty() || v.chars().any(char::is_control))
        || [
            ("ghcr.io/tailrocks/velnor-runner", &receipt.images.runner),
            ("ghcr.io/tailrocks/velnor-dind", &receipt.images.dind),
            (
                "ghcr.io/tailrocks/velnor-resource-probe",
                &receipt.images.resource_probe,
            ),
        ]
        .iter()
        .any(|(repository, image)| {
            image.repository != *repository
                || image.registry_digest != image.platform_manifest_digest
                || !digest(&image.registry_digest)
                || !digest(&image.config_digest)
                || !digest(&image.local_id)
        })
    {
        return Err(invalid());
    }
    Ok(receipt)
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| hex(value, 64))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
}

fn bounded_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
