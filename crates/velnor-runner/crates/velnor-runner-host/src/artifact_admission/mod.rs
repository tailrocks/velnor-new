// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use reqwest::Client;

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;

mod archive;

mod github;
mod hash;
mod inspection;
mod manifest;
mod projection_cache;
mod registry_projection;
mod registry_transport;
pub use registry_transport::{RegistryRequest, RegistryResponse, RegistryTransport};
mod runner_release;
mod strict_json;

pub use projection_cache::RegistryProjection;
pub(crate) use runner_release::VerifiedRunnerRelease;

#[derive(Clone, PartialEq, Eq)]
struct CacheKey {
    source_sha: String,
    helper_sha256: [u8; 32],
    state_directory: PathBuf,
}

static VERIFIED_RUNNER_RELEASE: OnceLock<tokio::sync::Mutex<Option<CachedRunnerRelease>>> =
    OnceLock::new();

struct CachedRunnerRelease {
    key: CacheKey,
    release: Arc<VerifiedRunnerRelease>,
}

pub(crate) struct NativeArtifactProvider<'a> {
    client: Client,
    pat: &'a str,
    identity: CompiledReleaseIdentity,
    key: CacheKey,
    state_directory: PathBuf,
}

pub(crate) struct ReleaseArchiveIdentity {
    pub(crate) oci_index_sha256: String,
    pub(crate) image_manifest_digest: String,
    pub(crate) config_digest: String,
}

pub(crate) const RELEASE_ARCHIVE_LIMIT: usize = archive::MAX_RELEASE_ARCHIVE_BYTES;
pub(crate) const RELEASE_MANIFEST_LIMIT: usize = manifest::MAX_MANIFEST_BYTES;

pub(crate) fn hash_release_bytes(bytes: &[u8]) -> String {
    hash::lowercase_hex(&hash::sha256(bytes))
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
    let probe_manifest = manifest::ProbeManifest {
        source_commit: source_commit.to_owned(),
        workflow_authority_sha: source_commit.to_owned(),
        platform: manifest::PLATFORM.to_owned(),
        archive_format: manifest::ARCHIVE_FORMAT.to_owned(),
        oci_index_sha256: archive_identity.oci_index_sha256.clone(),
        image_manifest_digest: archive_identity.image_manifest_digest.clone(),
        config_digest: archive_identity.config_digest.clone(),
        archive_sha256: hash::lowercase_hex(&hash::sha256(archive_bytes)),
    };
    inspection::validate_inspect(inspection, &probe_manifest, &archive_identity)?;
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
        let source_sha = hash::lowercase_hex(identity.source_sha());
        let key = CacheKey {
            source_sha,
            helper_sha256: *identity.helper_sha256(),
            state_directory: state_directory.to_path_buf(),
        };
        Ok(Self {
            client: github::client()?,
            pat,
            identity,
            key,
            state_directory: state_directory.to_path_buf(),
        })
    }

    pub(crate) async fn verified_runner_release(
        &self,
    ) -> Result<Arc<VerifiedRunnerRelease>, HostError> {
        let cache = VERIFIED_RUNNER_RELEASE.get_or_init(|| tokio::sync::Mutex::new(None));
        let mut guard = cache.lock().await;
        if let Some(cached) = guard.as_ref().filter(|cached| cached.key == self.key) {
            return Ok(Arc::clone(&cached.release));
        }
        let fetched = Arc::new(
            runner_release::fetch_verified(
                &self.client,
                self.pat,
                &self.identity,
                &self.state_directory,
            )
            .await?,
        );
        if fetched.source_sha != self.key.source_sha {
            return Err(HostError::Identity);
        }
        *guard = Some(CachedRunnerRelease {
            key: self.key.clone(),
            release: Arc::clone(&fetched),
        });
        Ok(fetched)
    }

    pub(crate) async fn verified_registry_projection(
        &self,
        role: velnor_runner_image_manifest::ImageKind,
    ) -> Result<RegistryProjection, HostError> {
        let release = self.verified_runner_release().await?;
        if release.source_sha != self.key.source_sha {
            return Err(HostError::Identity);
        }
        let image = match role {
            velnor_runner_image_manifest::ImageKind::Runner => &release.manifest.images.runner,
            velnor_runner_image_manifest::ImageKind::Dind => &release.manifest.images.dind,
            velnor_runner_image_manifest::ImageKind::ResourceProbe => {
                &release.manifest.images.resource_probe
            }
        };
        let mut cache = release.projections.lock().await;
        if let Some(projection) = cache.get(image, role, &release.source_sha) {
            return Ok(projection);
        }
        let projection =
            registry_projection::verify(&self.client, image, role, &release.source_sha).await?;
        cache.insert(image, projection.clone());
        Ok(projection)
    }
}

pub(crate) fn anonymous_registry_transport()
-> Result<std::sync::Arc<dyn RegistryTransport>, HostError> {
    Ok(std::sync::Arc::new(
        registry_transport::AnonymousRegistryTransport(github::client()?),
    ))
}

pub(crate) async fn verify_candidate_registry_projection(
    transport: &dyn RegistryTransport,
    image: &velnor_runner_image_manifest::ImageEntry,
    role: velnor_runner_image_manifest::ImageKind,
    source: &str,
) -> Result<RegistryProjection, HostError> {
    RegistryProjection::fetch(transport, image, role, source).await
}
