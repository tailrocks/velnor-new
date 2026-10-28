// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;
use sha2::{Digest, Sha256};

pub(crate) mod model;

use model::{
    DescriptorDocument, ImageEntries, ImageEntry, ResourceProbeProfile, RunnerImageManifest,
    TrustedSigningIdentity, Versions,
};

const REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_REF: &str = "refs/heads/main";
const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-images.yml";
const SIGNING_IDENTITY: &str = concat!(
    "https://github.com/tailrocks/velnor-new/",
    ".github/workflows/product-release-images.yml@refs/heads/main"
);
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const PLATFORM: &str = "linux/amd64";
const RUNNER_REPOSITORY: &str = "ghcr.io/tailrocks/velnor-runner";
const DIND_REPOSITORY: &str = "ghcr.io/tailrocks/velnor-dind";
const PROBE_REPOSITORY: &str = "ghcr.io/tailrocks/velnor-resource-probe";
const MANIFEST_LIMIT: usize = 64 * 1024;

#[derive(serde::Deserialize)]
struct RunnerLockVersions {
    #[serde(rename = "runner_version")]
    runner: String,
    #[serde(rename = "docker_version")]
    docker: String,
    #[serde(rename = "buildx_version")]
    buildx: String,
    #[serde(rename = "compose_version")]
    compose: String,
    #[serde(rename = "node_version")]
    node: String,
}

pub(super) fn write(
    descriptors_path: &Path,
    runner_lock_path: &Path,
    output_path: &Path,
) -> Result<(), HostError> {
    let manifest = load_manifest(descriptors_path, runner_lock_path)?;
    let mut bytes = serde_json::to_vec(&manifest).map_err(|_| HostError::Identity)?;
    bytes.push(b'\n');
    if bytes.len() > MANIFEST_LIMIT {
        return Err(HostError::Frame);
    }
    super::files::create_new_regular(output_path, &bytes)
}

fn load_manifest(
    descriptors_path: &Path,
    runner_lock_path: &Path,
) -> Result<RunnerImageManifest, HostError> {
    let descriptors_bytes = super::files::read_bounded_regular(descriptors_path, MANIFEST_LIMIT)?;
    let descriptors_value = crate::artifact_admission::parse_release_json(&descriptors_bytes)?;
    let descriptors: DescriptorDocument =
        serde_json::from_value(descriptors_value).map_err(|_| HostError::Identity)?;
    let source_commit = std::env::var("VELNOR_SOURCE_SHA").map_err(|_| HostError::Identity)?;
    let authority_sha =
        std::env::var("VELNOR_WORKFLOW_AUTHORITY_SHA").map_err(|_| HostError::Identity)?;
    super::manifest::validate_workflow_identity(&source_commit, &authority_sha)?;
    validate_descriptors(&descriptors, &source_commit)?;
    validate_images(&descriptors.images, &source_commit)?;
    let runner_lock = super::files::read_bounded_regular(runner_lock_path, MANIFEST_LIMIT)?;
    if sha256_hex(&runner_lock) != descriptors.runner_lock_sha256 {
        return Err(HostError::Identity);
    }
    let versions = read_versions(&runner_lock)?;
    Ok(manifest(
        descriptors.images,
        source_commit,
        authority_sha,
        versions,
    ))
}

fn validate_descriptors(
    descriptors: &DescriptorDocument,
    source_commit: &str,
) -> Result<(), HostError> {
    if descriptors.schema_version != 2
        || descriptors.source_commit != source_commit
        || descriptors.platform != PLATFORM
        || !is_lower_sha256(&descriptors.runner_lock_sha256)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_images(images: &ImageEntries, source_commit: &str) -> Result<(), HostError> {
    validate_image(&images.runner, RUNNER_REPOSITORY, source_commit)?;
    validate_image(&images.dind, DIND_REPOSITORY, source_commit)?;
    validate_image(&images.resource_probe, PROBE_REPOSITORY, source_commit)
}

fn validate_image(
    image: &ImageEntry,
    repository: &str,
    source_commit: &str,
) -> Result<(), HostError> {
    if image.repository != repository
        || image.tag != format!("sha-{source_commit}")
        || !is_digest(&image.registry_digest)
        || !is_digest(&image.platform_manifest_digest)
        || !is_digest(&image.config_digest)
        || image.registry_digest != image.platform_manifest_digest
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn read_versions(bytes: &[u8]) -> Result<Versions, HostError> {
    let lock_text = std::str::from_utf8(bytes).map_err(|_| HostError::Identity)?;
    let lock: RunnerLockVersions = toml::from_str(lock_text).map_err(|_| HostError::Identity)?;
    if [
        &lock.runner,
        &lock.docker,
        &lock.buildx,
        &lock.compose,
        &lock.node,
    ]
    .iter()
    .any(|version| !velnor_actions_contract::config::is_exact_semver(version))
    {
        return Err(HostError::Identity);
    }
    Ok(Versions {
        runner: lock.runner,
        docker: lock.docker,
        buildx: lock.buildx,
        compose: lock.compose,
        node: lock.node,
    })
}

fn manifest(
    images: ImageEntries,
    source_commit: String,
    authority_sha: String,
    versions: Versions,
) -> RunnerImageManifest {
    RunnerImageManifest {
        schema_version: 2,
        repository: REPOSITORY.to_owned(),
        source_ref: SOURCE_REF.to_owned(),
        signer_workflow: SIGNER_WORKFLOW.to_owned(),
        signer_ref: SOURCE_REF.to_owned(),
        trusted_signing_identity: TrustedSigningIdentity {
            oidc_issuer: OIDC_ISSUER.to_owned(),
            certificate_identity: SIGNING_IDENTITY.to_owned(),
        },
        source_commit,
        workflow_authority_sha: authority_sha,
        platform: PLATFORM.to_owned(),
        versions,
        images,
        resource_probe: ResourceProbeProfile {
            protocol_version: 1,
            image_user: "65532:65532".to_owned(),
            entrypoint: "/velnor/resource-probe".to_owned(),
            working_directory: "/".to_owned(),
        },
    }
}

fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
