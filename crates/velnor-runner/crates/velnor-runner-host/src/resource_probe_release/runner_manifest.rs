// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use sha2::{Digest, Sha256};

use velnor_runner_image_manifest::{
    ImageEntries, ImageEntry, ResourceProbeProfile, RunnerImageManifest, TrustedSigningIdentity,
    Versions,
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

pub(crate) fn parse_for_source(
    bytes: &[u8],
    source_commit: &str,
    runner_lock: &[u8],
) -> Result<RunnerImageManifest, HostError> {
    if bytes.is_empty() || bytes.len() > MANIFEST_LIMIT || runner_lock.len() > MANIFEST_LIMIT {
        return Err(HostError::Frame);
    }
    if source_commit.len() != 40
        || !source_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::Identity);
    }
    let value =
        velnor_runner_image_manifest::strict_json::parse_strict_json_bytes(bytes, MANIFEST_LIMIT)
            .map_err(|_| HostError::Identity)?;
    let parsed: RunnerImageManifest =
        serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    validate_images(&parsed.images, source_commit)?;
    let actual = serde_json::to_value(&parsed).map_err(|_| HostError::Identity)?;
    let expected = manifest(
        parsed.images,
        source_commit.to_owned(),
        source_commit.to_owned(),
        sha256_hex(runner_lock),
        read_versions(runner_lock)?,
    );
    if actual != serde_json::to_value(&expected).map_err(|_| HostError::Identity)? {
        return Err(HostError::Identity);
    }
    Ok(expected)
}

fn validate_images(images: &ImageEntries, source_commit: &str) -> Result<(), HostError> {
    validate_image(
        &images.runner,
        RUNNER_REPOSITORY,
        source_commit,
        "images/runner/ubuntu-26.04",
        &[
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ],
    )?;
    validate_image(
        &images.dind,
        DIND_REPOSITORY,
        source_commit,
        "images/dind",
        &["resource_probe"],
    )?;
    validate_image(
        &images.resource_probe,
        PROBE_REPOSITORY,
        source_commit,
        "images/resource-probe",
        &["resource_probe"],
    )
}

fn validate_image(
    image: &ImageEntry,
    repository: &str,
    source_commit: &str,
    context: &str,
    names: &[&str],
) -> Result<(), HostError> {
    if image.repository != repository
        || image.tag != format!("sha-{source_commit}")
        || !is_digest(&image.registry_digest)
        || !is_digest(&image.platform_manifest_digest)
        || !is_digest(&image.config_digest)
        || image.registry_digest != image.platform_manifest_digest
        || image.config_digest == image.registry_digest
        || !is_lower_sha256(&image.build_source.metadata_sha256)
        || image.build_source.build_type
            != "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md"
        || image.build_source.source_repository != "https://github.com/tailrocks/velnor-new"
        || image.build_source.source_commit != source_commit
        || image.build_source.context_path != context
        || image.build_source.dockerfile_path != format!("{context}/Dockerfile")
        || image.build_source.builder_platform != PLATFORM
        || image.build_source.named_contexts.len() != names.len()
        || !names.iter().all(|name| {
            image
                .build_source
                .named_contexts
                .get(*name)
                .is_some_and(|digest| is_lower_sha256(digest))
        })
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
    .any(|version| !super::is_exact_semver(version))
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
    runner_lock_sha256: String,
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
        runner_lock_sha256,
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
