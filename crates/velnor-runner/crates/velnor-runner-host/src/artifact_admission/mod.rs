//! Source-bound release admission for the controller-owned resource probe.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use reqwest::Client;

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;

pub(crate) const MAX_WORKER_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

mod archive;
mod checksums;
#[cfg(test)]
mod docker;
mod github;
mod hash;
mod inspection;
mod manifest;
mod release;
mod strict_json;

#[derive(Clone, PartialEq, Eq)]
struct CacheKey {
    source_sha: String,
    helper_sha256: [u8; 32],
    state_directory: PathBuf,
}

struct CachedRelease {
    key: CacheKey,
    release: Arc<release::VerifiedRelease>,
}

static VERIFIED_RELEASE: OnceLock<tokio::sync::Mutex<Option<CachedRelease>>> = OnceLock::new();

pub(crate) struct NativeArtifactProvider<'a> {
    client: Client,
    pat: &'a str,
    identity: CompiledReleaseIdentity,
    key: CacheKey,
    state_directory: PathBuf,
}

pub(crate) struct ProbeReleaseMaterial {
    pub(crate) source_sha: String,
    pub(crate) archive: Vec<u8>,
    pub(crate) manifest: Vec<u8>,
    pub(crate) checksums: Vec<u8>,
}

pub(crate) struct WorkerReleaseMaterial {
    pub(crate) source_sha: String,
    pub(crate) runner_path: PathBuf,
    pub(crate) runner_sha256: String,
    pub(crate) dind_path: PathBuf,
    pub(crate) dind_sha256: String,
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

    pub(crate) async fn verified_probe_material(&self) -> Result<ProbeReleaseMaterial, HostError> {
        let release = self.verified_release().await?;
        Ok(ProbeReleaseMaterial {
            source_sha: release.source_sha.clone(),
            archive: release.archive.clone(),
            manifest: release.manifest_bytes.clone(),
            checksums: release.checksum_bytes.clone(),
        })
    }

    pub(crate) async fn download_worker_material(
        &self,
        directory: &Path,
    ) -> Result<WorkerReleaseMaterial, HostError> {
        let release = self.verified_release().await?;
        let metadata = std::fs::symlink_metadata(directory).map_err(|_| HostError::Path)?;
        if !directory.is_absolute()
            || !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || release.runner_asset.name != crate::artifact_admission::manifest::RUNNER_ASSET
            || release.dind_asset.name != crate::artifact_admission::manifest::DIND_ASSET
        {
            return Err(HostError::Identity);
        }
        let runner_path = directory.join(&release.runner_asset.name);
        let dind_path = directory.join(&release.dind_asset.name);
        github::download_asset_to_path(
            &self.client,
            self.pat,
            &release.runner_asset,
            &runner_path,
            MAX_WORKER_ARCHIVE_BYTES,
        )
        .await?;
        github::download_asset_to_path(
            &self.client,
            self.pat,
            &release.dind_asset,
            &dind_path,
            MAX_WORKER_ARCHIVE_BYTES,
        )
        .await?;
        Ok(WorkerReleaseMaterial {
            source_sha: release.source_sha.clone(),
            runner_path,
            runner_sha256: release
                .runner_asset
                .digest
                .strip_prefix("sha256:")
                .ok_or(HostError::Identity)?
                .to_owned(),
            dind_path,
            dind_sha256: release
                .dind_asset
                .digest
                .strip_prefix("sha256:")
                .ok_or(HostError::Identity)?
                .to_owned(),
        })
    }

    async fn verified_release(&self) -> Result<Arc<release::VerifiedRelease>, HostError> {
        let cache = VERIFIED_RELEASE.get_or_init(|| tokio::sync::Mutex::new(None));
        let mut guard = cache.lock().await;
        if let Some(release) = cached_release(guard.as_ref(), &self.key) {
            return Ok(release);
        }
        let fetched = Arc::new(
            release::fetch_verified(
                &self.client,
                self.pat,
                &self.identity,
                &self.state_directory,
            )
            .await?,
        );
        validate_fetched_release(&self.key, &fetched)?;
        *guard = Some(CachedRelease {
            key: self.key.clone(),
            release: Arc::clone(&fetched),
        });
        Ok(fetched)
    }
}

fn cached_release(
    cached: Option<&CachedRelease>,
    requested: &CacheKey,
) -> Option<Arc<release::VerifiedRelease>> {
    cached
        .filter(|cached| &cached.key == requested)
        .map(|cached| Arc::clone(&cached.release))
}

fn validate_fetched_release(
    key: &CacheKey,
    fetched: &release::VerifiedRelease,
) -> Result<(), HostError> {
    if fetched.source_sha != key.source_sha
        || fetched.authority_sha != key.source_sha
        || fetched.archive_sha != fetched.manifest.archive_sha256
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

#[cfg(test)]
struct RuntimeImage {
    runtime_id: String,
}

#[cfg(test)]
fn image_binding_fingerprint(
    release: &release::VerifiedRelease,
    engine_id: &str,
    docker_root: &str,
    runtime_id: &str,
) -> String {
    use sha2::{Digest, Sha256};

    let mut digest = Sha256::new();
    digest.update(b"velnor-resource-probe-image-binding-v1\0");
    for field in [
        release.source_sha.as_bytes(),
        release.authority_sha.as_bytes(),
        release.archive_sha.as_bytes(),
        manifest::PLATFORM.as_bytes(),
        engine_id.as_bytes(),
        docker_root.as_bytes(),
        runtime_id.as_bytes(),
    ] {
        let size = u64::try_from(field.len()).unwrap_or(u64::MAX);
        digest.update(size.to_be_bytes());
        digest.update(field);
    }
    hash::lowercase_hex(&digest.finalize())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
