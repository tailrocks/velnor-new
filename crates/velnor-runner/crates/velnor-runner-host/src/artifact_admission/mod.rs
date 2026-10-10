//! Source-bound release admission for the controller-owned resource probe.

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
}

pub(crate) struct ArtifactCandidate {
    pub(crate) runtime_id: String,
    pub(crate) platform: String,
    pub(crate) source_revision: String,
    pub(crate) binding_fingerprint: String,
    pub(crate) config: ContainerConfig,
}

impl<'a> NativeArtifactProvider<'a> {
    pub(crate) fn from_compiled_release_identity(pat: &'a str) -> Result<Self, HostError> {
        let identity =
            crate::compile_identity::compiled_release_identity().ok_or(HostError::Identity)?;
        if !github::valid_pat(pat) {
            return Err(HostError::Identity);
        }
        let source_sha = hash::lowercase_hex(identity.source_sha());
        let key = CacheKey {
            source_sha,
            helper_sha256: *identity.helper_sha256(),
        };
        Ok(Self {
            client: github::client()?,
            pat,
            identity,
            key,
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
        if let Some(cached) = guard.as_ref()
            && cached.key == self.key
        {
            return Ok(Arc::clone(&cached.release));
        }
        let fetched =
            Arc::new(release::fetch_verified(&self.client, self.pat, &self.identity).await?);
        if fetched.source_sha != self.key.source_sha
            || fetched.authority_sha != self.key.source_sha
            || fetched.archive_sha != fetched.manifest.archive_sha256
        {
            return Err(HostError::Identity);
        }
        *guard = Some(CachedRelease {
            key: self.key.clone(),
            release: Arc::clone(&fetched),
        });
        Ok(fetched)
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

#[cfg(test)]
mod tests {
    use super::image_binding_fingerprint;
    use crate::artifact_admission::release::VerifiedRelease;
    use crate::error::HostError;

    #[test]
    fn fingerprint_binds_archive_engine_root_and_runtime_id() -> Result<(), HostError> {
        let manifest = crate::artifact_admission::manifest::ProbeManifest {
            source_commit: "a".repeat(40),
            workflow_authority_sha: "a".repeat(40),
            platform: crate::artifact_admission::manifest::PLATFORM.to_owned(),
            archive_format: crate::artifact_admission::manifest::ARCHIVE_FORMAT.to_owned(),
            oci_index_sha256: "b".repeat(64),
            image_manifest_digest: format!("sha256:{}", "c".repeat(64)),
            config_digest: format!("sha256:{}", "d".repeat(64)),
            archive_sha256: "e".repeat(64),
        };
        let release = VerifiedRelease {
            source_sha: manifest.source_commit.clone(),
            authority_sha: manifest.workflow_authority_sha.clone(),
            archive_sha: manifest.archive_sha256.clone(),
            archive: Vec::new(),
            manifest,
            archive_identity: crate::artifact_admission::archive::ArchiveIdentity {
                image_manifest_digest: format!("sha256:{}", "c".repeat(64)),
                image_manifest_media_type: "application/vnd.oci.image.manifest.v1+json".to_owned(),
                image_manifest_size: 0,
                config_digest: format!("sha256:{}", "d".repeat(64)),
                config: bollard::models::ContainerConfig::default(),
            },
        };
        let expected = image_binding_fingerprint(&release, "engine", "/data", "sha256:runtime");
        if expected == image_binding_fingerprint(&release, "other", "/data", "sha256:runtime")
            || expected == image_binding_fingerprint(&release, "engine", "/other", "sha256:runtime")
            || expected == image_binding_fingerprint(&release, "engine", "/data", "sha256:other")
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}
