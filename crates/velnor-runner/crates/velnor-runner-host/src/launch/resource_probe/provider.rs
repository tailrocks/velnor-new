//! Verified resource-probe image acquisition and daemon binding.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{ContainerConfig, ImageInspect, SystemInfo};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OnceCell};

use crate::error::HostError;

use super::projection::DockerRoot;

mod archive;
mod gh;
mod release;
#[cfg(test)]
mod tests;

const SOURCE_REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_REF: &str = "refs/heads/main";
const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-images.yml";
const SIGNER_IDENTITY: &str = "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main";
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const PLATFORM: &str = "linux/amd64";
const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const IMAGE_TAG: &str = "velnor-resource-probe:linux-amd64";
const PREPARE_TIMEOUT: Duration = Duration::from_secs(180);

/// A verified release image plus the selected daemon's inspected image config.
pub(super) struct VerifiedCandidate {
    pub(super) runtime_id: String,
    /// Verified probe image target. This can differ from daemon architecture under emulation.
    pub(super) platform: String,
    /// Canonical architecture reported by the selected Docker engine.
    pub(super) daemon_architecture: String,
    pub(super) source_revision: String,
    pub(super) binding_fingerprint: String,
    pub(super) config: ContainerConfig,
}

/// Supplies only an inspected daemon-specific immutable image identity.
pub(super) trait ImageProvider {
    /// Authenticate, verify, load offline, and inspect the selected daemon's image.
    async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError>;
}

/// GitHub release provider pinned to the source SHA compiled into the host.
pub(super) struct ReleaseImageProvider<'a> {
    pat: &'a str,
    source_revision: String,
}

impl<'a> ReleaseImageProvider<'a> {
    /// Construct only when the release build supplied its immutable source SHA.
    pub(super) fn from_compiled_source(pat: &'a str) -> Result<Self, ProviderFailure> {
        let source_revision = compiled_source_revision()?;
        Self::with_source_revision(pat, source_revision)
    }

    fn with_source_revision(pat: &'a str, source_revision: &str) -> Result<Self, ProviderFailure> {
        if pat.trim().is_empty() {
            return Err(ProviderFailure::CredentialMissing);
        }
        if !valid_sha(source_revision) {
            return Err(ProviderFailure::TrustedSourcePinInvalid);
        }
        Ok(Self {
            pat,
            source_revision: source_revision.to_owned(),
        })
    }

    async fn verified_release(&self) -> Result<Arc<release::PreparedRelease>, ProviderFailure> {
        let cells = PREPARED_RELEASES.get_or_init(|| Mutex::new(HashMap::new()));
        let cell = {
            let mut cells = cells.lock().await;
            Arc::clone(
                cells
                    .entry(self.source_revision.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        let prepared = cell
            .get_or_try_init(|| async {
                release::prepare(self.pat, &self.source_revision)
                    .await
                    .map(Arc::new)
            })
            .await?;
        Ok(Arc::clone(prepared))
    }

    async fn candidate_from_release<F>(
        &self,
        docker: &Docker,
        selected: &SelectedDaemon,
        release: F,
    ) -> Result<VerifiedCandidate, ProviderFailure>
    where
        F: Future<Output = Result<Arc<release::PreparedRelease>, ProviderFailure>>,
    {
        let release = release.await?;
        verify_selected_daemon(docker, selected).await?;
        let image = self.inspect_or_load(docker, selected, &release).await?;
        candidate(&selected.engine_id, &selected.architecture, image, &release)
    }

    async fn verified_candidate_with_release<F>(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &SystemInfo,
        release: F,
    ) -> Result<Option<VerifiedCandidate>, ProviderFailure>
    where
        F: Future<Output = Result<Arc<release::PreparedRelease>, ProviderFailure>>,
    {
        if !daemon_platform_matches(info) {
            return Ok(None);
        }
        let selected = selected_daemon(info, engine_id)?;
        let candidate = tokio::time::timeout(
            PREPARE_TIMEOUT,
            self.candidate_from_release(docker, &selected, release),
        )
        .await
        .map_err(|_| ProviderFailure::Timeout)??;
        Ok(Some(candidate))
    }

    async fn inspect_or_load(
        &self,
        docker: &Docker,
        selected: &SelectedDaemon,
        release: &release::PreparedRelease,
    ) -> Result<ImageInspect, ProviderFailure> {
        for reference in [
            release.config_digest.as_str(),
            release.manifest_digest.as_str(),
        ] {
            if let Some(image) = inspect_image(docker, reference).await? {
                verify_daemon_image(&image, release)?;
                return Ok(image);
            }
        }

        verify_selected_daemon(docker, selected).await?;
        let response = docker.import_image(
            bollard::query_parameters::ImportImageOptionsBuilder::default().build(),
            bollard::body_full(release.archive.clone().into()),
            None,
        );
        tokio::time::timeout(PREPARE_TIMEOUT, async {
            futures_util::pin_mut!(response);
            while let Some(item) = futures_util::StreamExt::next(&mut response).await {
                item.map_err(|_| ProviderFailure::ImageLoad)?;
            }
            Ok::<(), ProviderFailure>(())
        })
        .await
        .map_err(|_| ProviderFailure::ImageLoad)??;
        verify_selected_daemon(docker, selected).await?;
        let image = inspect_image(docker, IMAGE_TAG)
            .await?
            .ok_or(ProviderFailure::ImageInspect)?;
        verify_daemon_image(&image, release)?;
        Ok(image)
    }
}

impl ImageProvider for ReleaseImageProvider<'_> {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        let result = self
            .verified_candidate_with_release(docker, engine_id, info, self.verified_release())
            .await;
        match result {
            Ok(Some(candidate)) => Ok(Some(candidate)),
            Ok(None) => {
                eprintln!("resource_probe=unavailable reason=selected_daemon_platform_mismatch");
                Ok(None)
            }
            Err(error) => {
                eprintln!("resource_probe=unavailable reason={}", error.code());
                Err(HostError::Docker)
            }
        }
    }
}

/// Fail-closed provider used only by lifecycle tests that expect no image.
#[cfg(test)]
pub(super) struct UnavailableImageProvider;

#[cfg(test)]
impl ImageProvider for UnavailableImageProvider {
    async fn verified_candidate(
        &self,
        _docker: &Docker,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        std::future::ready(Ok(None)).await
    }
}

type PreparedReleaseCell = Arc<OnceCell<Arc<release::PreparedRelease>>>;

static PREPARED_RELEASES: OnceLock<Mutex<HashMap<String, PreparedReleaseCell>>> = OnceLock::new();

struct SelectedDaemon {
    engine_id: String,
    docker_root_digest: String,
    architecture: String,
}

fn selected_daemon(info: &SystemInfo, engine_id: &str) -> Result<SelectedDaemon, ProviderFailure> {
    if info.id.as_deref() != Some(engine_id) || !daemon_platform_matches(info) {
        return Err(ProviderFailure::ImageIdentity);
    }
    let root = info
        .docker_root_dir
        .as_deref()
        .ok_or(ProviderFailure::ImageIdentity)?;
    let root = DockerRoot::parse(root).map_err(|_| ProviderFailure::ImageIdentity)?;
    let architecture = daemon_architecture(info).ok_or(ProviderFailure::ImageIdentity)?;
    Ok(SelectedDaemon {
        engine_id: engine_id.to_owned(),
        docker_root_digest: root.digest().to_owned(),
        architecture: architecture.to_owned(),
    })
}

async fn verify_selected_daemon(
    docker: &Docker,
    selected: &SelectedDaemon,
) -> Result<(), ProviderFailure> {
    let info = tokio::time::timeout(Duration::from_secs(20), docker.info())
        .await
        .map_err(|_| ProviderFailure::DaemonIdentity)?
        .map_err(|_| ProviderFailure::DaemonIdentity)?;
    let current = selected_daemon(&info, &selected.engine_id)?;
    if current.engine_id == selected.engine_id
        && current.docker_root_digest == selected.docker_root_digest
        && current.architecture == selected.architecture
    {
        Ok(())
    } else {
        Err(ProviderFailure::ImageIdentity)
    }
}

pub(super) fn daemon_platform_matches(info: &SystemInfo) -> bool {
    info.os_type.as_deref() == Some("linux") && daemon_architecture(info).is_some()
}

pub(super) fn daemon_architecture(info: &SystemInfo) -> Option<&'static str> {
    match info.architecture.as_deref()? {
        "x86_64" | "amd64" => Some("x86_64"),
        "aarch64" | "arm64" => Some("aarch64"),
        _ => None,
    }
}

fn compiled_source_revision() -> Result<&'static str, ProviderFailure> {
    let source_revision =
        option_env!("VELNOR_SOURCE_SHA").ok_or(ProviderFailure::TrustedSourcePinMissing)?;
    if valid_sha(source_revision) {
        Ok(source_revision)
    } else {
        Err(ProviderFailure::TrustedSourcePinInvalid)
    }
}

pub(super) fn build_pin_status() -> Result<(), &'static str> {
    compiled_source_revision()
        .map(|_| ())
        .map_err(ProviderFailure::code)
}

async fn inspect_image(
    docker: &Docker,
    reference: &str,
) -> Result<Option<ImageInspect>, ProviderFailure> {
    match tokio::time::timeout(Duration::from_secs(20), docker.inspect_image(reference)).await {
        Ok(Ok(image)) => Ok(Some(image)),
        Ok(Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        })) => Ok(None),
        Ok(Err(_)) | Err(_) => Err(ProviderFailure::ImageInspect),
    }
}

fn verify_daemon_image(
    image: &ImageInspect,
    release: &release::PreparedRelease,
) -> Result<(), ProviderFailure> {
    let id = image.id.as_deref().ok_or(ProviderFailure::ImageIdentity)?;
    if !digest_reference(id)
        || !store_identity_matches(image, id, release)
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    let config = image.config.as_ref().ok_or(ProviderFailure::ImageConfig)?;
    let actual = serde_json::to_value(config).map_err(|_| ProviderFailure::ImageConfig)?;
    let expected =
        serde_json::to_value(&release.config).map_err(|_| ProviderFailure::ImageConfig)?;
    if actual != expected {
        return Err(ProviderFailure::ImageConfig);
    }
    Ok(())
}

fn store_identity_matches(
    image: &ImageInspect,
    id: &str,
    release: &release::PreparedRelease,
) -> bool {
    match image.descriptor.as_ref() {
        None => id == release.config_digest && image.manifests.is_none(),
        Some(descriptor) => {
            let expected_size = i64::try_from(release.manifest_size).ok();
            let expected_annotations = HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    "docker.io/library/velnor-resource-probe:linux-amd64".to_owned(),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    "linux-amd64".to_owned(),
                ),
            ]);
            id == release.manifest_digest
                && descriptor.digest.as_deref() == Some(release.manifest_digest.as_str())
                && descriptor.media_type.as_deref() == Some(OCI_IMAGE_MANIFEST_MEDIA_TYPE)
                && descriptor.size == expected_size
                && descriptor.annotations.as_ref() == Some(&expected_annotations)
                && descriptor.urls.is_none()
                && descriptor.data.is_none()
                && descriptor.platform.is_none()
                && image.manifests.is_none()
        }
    }
}

fn candidate(
    engine_id: &str,
    daemon_architecture: &str,
    image: ImageInspect,
    release: &release::PreparedRelease,
) -> Result<VerifiedCandidate, ProviderFailure> {
    verify_daemon_image(&image, release)?;
    let runtime_id = image.id.ok_or(ProviderFailure::ImageIdentity)?;
    let config = serde_json::from_value(
        serde_json::to_value(image.config.ok_or(ProviderFailure::ImageConfig)?)
            .map_err(|_| ProviderFailure::ImageConfig)?,
    )
    .map_err(|_| ProviderFailure::ImageConfig)?;
    let binding_fingerprint = image_binding(engine_id, daemon_architecture, &runtime_id, release);
    Ok(VerifiedCandidate {
        runtime_id,
        platform: PLATFORM.to_owned(),
        daemon_architecture: daemon_architecture.to_owned(),
        source_revision: release.source_revision.clone(),
        binding_fingerprint,
        config,
    })
}

fn image_binding(
    engine_id: &str,
    daemon_architecture: &str,
    runtime_id: &str,
    release: &release::PreparedRelease,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-resource-probe-image-binding-v1\0");
    for value in [
        engine_id,
        daemon_architecture,
        &release.source_revision,
        PLATFORM,
        &release.index_digest,
        &release.manifest_digest,
        &release.config_digest,
        runtime_id,
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

fn digest_reference(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex_value| valid_hex(hex_value, 64))
}

fn valid_sha(value: &str) -> bool {
    valid_hex(value, 40)
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderFailure {
    TrustedSourcePinMissing,
    TrustedSourcePinInvalid,
    CredentialMissing,
    GhUnavailable,
    GhVersion,
    GhCommand,
    GhTimeout,
    GhOutput,
    ReleaseMetadata,
    ReleaseSignature,
    ReleaseTarget,
    ReleaseAssets,
    AssetChecksum,
    Attestation,
    Manifest,
    Archive,
    ImageLoad,
    ImageInspect,
    ImageIdentity,
    ImageConfig,
    DaemonIdentity,
    Timeout,
}

impl ProviderFailure {
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::TrustedSourcePinMissing => "trusted_source_pin_missing",
            Self::TrustedSourcePinInvalid => "trusted_source_pin_invalid",
            Self::CredentialMissing => "credential_missing",
            Self::GhUnavailable => "gh_unavailable",
            Self::GhVersion => "gh_version_mismatch",
            Self::GhCommand => "release_verification_failed",
            Self::GhTimeout => "release_verification_timeout",
            Self::GhOutput => "release_verification_output_invalid",
            Self::ReleaseMetadata => "release_metadata_invalid",
            Self::ReleaseSignature => "release_signature_invalid",
            Self::ReleaseTarget => "release_source_mismatch",
            Self::ReleaseAssets => "release_assets_mismatch",
            Self::AssetChecksum => "release_asset_checksum_mismatch",
            Self::Attestation => "release_attestation_mismatch",
            Self::Manifest => "resource_probe_manifest_mismatch",
            Self::Archive => "resource_probe_archive_mismatch",
            Self::ImageLoad => "selected_daemon_image_load_failed",
            Self::ImageInspect => "selected_daemon_image_inspect_failed",
            Self::ImageIdentity => "selected_daemon_image_identity_mismatch",
            Self::ImageConfig => "selected_daemon_image_config_mismatch",
            Self::DaemonIdentity => "selected_daemon_identity_unavailable",
            Self::Timeout => "provider_timeout",
        }
    }
}
