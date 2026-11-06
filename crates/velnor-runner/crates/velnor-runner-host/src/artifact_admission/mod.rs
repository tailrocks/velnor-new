// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use bollard::models::ContainerConfig;
use reqwest::Client;

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;

mod archive;
mod background;
mod bootstrap;
pub(crate) use background::{BootstrapSlot, start as start_bootstrap};
pub use bootstrap::{AdmittedImages, admit_runner_images};
mod docker;
mod github;
mod hash;
mod manifest;
mod registry;
mod runner_docker;
mod runner_release;
mod strict_json;

pub(crate) struct NativeArtifactProvider<'a> {
    client: Client,
    pat: &'a str,
    identity: CompiledReleaseIdentity,
    state_directory: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct ArtifactCandidate {
    pub(crate) runtime_id: String,
    pub(crate) platform: String,
    pub(crate) source_revision: String,
    pub(crate) binding_fingerprint: String,
    pub(crate) config: ContainerConfig,
}

pub(crate) struct ReleaseArchiveIdentity {
    pub(crate) oci_index_sha256: String,
    pub(crate) image_manifest_digest: String,
    pub(crate) config_digest: String,
}

pub(crate) const RELEASE_ARCHIVE_LIMIT: usize = archive::MAX_RELEASE_ARCHIVE_BYTES;
pub(crate) const RELEASE_MANIFEST_LIMIT: usize = manifest::MAX_MANIFEST_BYTES;

pub(crate) fn valid_runtime_image_id(value: &str) -> bool {
    hash::is_sha256_digest(value)
}

pub(crate) fn hash_release_bytes(bytes: &[u8]) -> String {
    hash::lowercase_hex(&hash::sha256(bytes))
}

pub(crate) fn parse_release_json(bytes: &[u8]) -> Result<serde_json::Value, HostError> {
    strict_json::parse(bytes)
}

pub(crate) fn inspect_release_archive(
    inspect_bytes: &[u8],
    archive_bytes: &[u8],
    source_commit: &str,
) -> Result<ReleaseArchiveIdentity, HostError> {
    use bollard::models::ImageInspect;

    if inspect_bytes.is_empty() || inspect_bytes.len() > RELEASE_MANIFEST_LIMIT {
        return Err(HostError::Frame);
    }
    let archive_identity = archive::inspect_for_release(archive_bytes, source_commit)?;
    let inspect_value = strict_json::parse(inspect_bytes)?;
    let inspections = inspect_value
        .as_array()
        .filter(|entries| entries.len() == 1)
        .ok_or(HostError::Identity)?;
    let inspection_value = inspections.first().ok_or(HostError::Identity)?;
    if inspection_value
        .get("Descriptor")
        .is_some_and(|descriptor| !descriptor.is_object())
    {
        return Err(HostError::Identity);
    }
    let inspection: ImageInspect =
        serde_json::from_value(inspection_value.clone()).map_err(|_| HostError::Identity)?;
    docker::validate_inspect(inspection, &archive_identity)?;
    Ok(ReleaseArchiveIdentity {
        oci_index_sha256: archive_identity.oci_index_sha256,
        image_manifest_digest: archive_identity.image_manifest_digest,
        config_digest: archive_identity.config_digest,
    })
}

impl<'a> NativeArtifactProvider<'a> {
    pub(crate) fn from_compiled_release_identity(
        pat: &'a str,
        state_directory: &Path,
    ) -> Result<Self, HostError> {
        let identity =
            crate::compile_identity::compiled_release_identity().ok_or(HostError::Identity)?;
        if !state_directory.is_absolute() {
            return Err(HostError::Path);
        }
        if !github::valid_pat(pat) {
            return Err(HostError::Identity);
        }
        Ok(Self {
            client: github::client()?,
            pat,
            identity,
            state_directory: state_directory.to_path_buf(),
        })
    }
}

struct RuntimeImage {
    runtime_id: String,
    config: ContainerConfig,
}
