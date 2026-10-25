use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use bollard::Docker;
use bollard::models::{ContainerConfig, SystemInfo};
use reqwest::Client;
use tokio::time::{Duration, Instant, timeout_at};

use crate::compile_identity::CompiledReleaseIdentity;
use crate::error::HostError;

mod archive;
mod checksums;
mod docker;
mod github;
mod hash;
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
    docker::validate_inspect(inspection, &probe_manifest, &archive_identity)?;
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

    pub(crate) async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<ArtifactCandidate>, HostError> {
        let deadline = Instant::now() + Duration::from_secs(27);
        let release = timeout_at(deadline, self.verified_release())
            .await
            .map_err(|_| HostError::DockerTimeout)??;
        if deadline.saturating_duration_since(Instant::now()) <= Duration::from_secs(5) {
            return Err(HostError::DockerTimeout);
        }
        let root = info.docker_root_dir.as_deref().ok_or(HostError::Path)?;
        if info.id.as_deref() != Some(engine_id) || root.is_empty() {
            return Err(HostError::Identity);
        }
        let runtime = docker::load_and_inspect(docker, engine_id, root, &release, deadline).await?;
        let fingerprint = image_binding_fingerprint(&release, engine_id, root, &runtime.runtime_id);
        Ok(Some(ArtifactCandidate {
            runtime_id: runtime.runtime_id,
            platform: manifest::PLATFORM.to_owned(),
            source_revision: release.source_sha.clone(),
            binding_fingerprint: fingerprint,
            config: runtime.config,
        }))
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

struct RuntimeImage {
    runtime_id: String,
    config: ContainerConfig,
}

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
