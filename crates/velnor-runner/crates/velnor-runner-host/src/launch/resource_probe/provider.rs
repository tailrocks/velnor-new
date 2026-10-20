//! Verified resource-probe image acquisition and daemon binding.

use std::path::PathBuf;
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ImageInspect, SystemInfo};

use crate::artifact_admission::{NativeArtifactProvider, WorkerReleaseMaterial};
use crate::error::HostError;
use crate::journal::{ImageImportState, Journal};

mod archive;
mod evidence;
mod failure;
mod identity;
mod release;
mod release_provider;
#[cfg(test)]
mod tests;
mod worker_import;
mod worker_prepare;
mod worker_stream;
mod worker_verify;
mod workspace;

pub(in crate::launch) use evidence::ProbeImagePreparationEvidence;
pub(super) use evidence::VerifiedCandidate;
pub(super) use failure::ProviderFailure;
use identity::{
    SelectedDaemon, candidate, compiled_source_revision, image_import_request, inspect_image,
    selected_daemon, verified_import_inspection, verify_daemon_image, verify_selected_daemon,
};
pub(in crate::launch) use worker_import::VerifiedWorkerImageIdentityProof;
pub(in crate::launch) use workspace::VerifiedWorkspaceCleanup;

const SOURCE_REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_REF: &str = "refs/heads/main";
const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-images.yml";
const SIGNER_IDENTITY: &str = "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main";
const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const PLATFORM: &str = "linux/amd64";
const IMAGE_TAG: &str = "velnor-resource-probe:linux-amd64";
const PREPARE_TIMEOUT: Duration = Duration::from_secs(180);

pub(super) fn build_pin_status() -> Result<(), &'static str> {
    identity::build_pin_status()
}

pub(super) fn daemon_platform_matches(info: &SystemInfo) -> bool {
    identity::daemon_platform_matches(info)
}

pub(super) fn daemon_architecture(info: &SystemInfo) -> Option<&'static str> {
    identity::daemon_architecture(info)
}

pub(super) async fn prepare_worker_images(
    docker: &Docker,
    journal: &Journal,
    expected: &crate::launch::SelectedEngineContext,
    release_read_secret: &str,
) -> Result<crate::launch::VerifiedWorkerImages, HostError> {
    worker_prepare::prepare_worker_images(docker, journal, expected, release_read_secret).await
}

pub(super) async fn verify_worker_images_for_engine(
    docker: &Docker,
    expected: &crate::launch::SelectedEngineContext,
    images: &crate::launch::VerifiedWorkerImages,
) -> Result<(), HostError> {
    worker_verify::verify_worker_images_for_engine(docker, expected, images)
        .await
        .map_err(|error| {
            eprintln!("worker_images=unavailable reason={}", error.code());
            HostError::Docker
        })
}

/// Supplies only an inspected daemon-specific immutable image identity.
pub(in crate::launch) trait ImageProvider {
    /// Authenticate, verify, load offline, and inspect the selected daemon's image.
    async fn verified_candidate(
        &self,
        docker: &Docker,
        journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError>;
}

/// GitHub release provider pinned to the source SHA compiled into the host.
pub(super) struct ReleaseImageProvider<'a> {
    native: Option<NativeArtifactProvider<'a>>,
    source_revision: String,
    cache_key: Option<PreparedReleaseCacheKey>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct PreparedReleaseCacheKey {
    source_revision: String,
    helper_sha256: [u8; 32],
    state_directory: PathBuf,
}

impl<'a> ReleaseImageProvider<'a> {
    /// Construct only when the release build supplied its immutable source SHA.
    pub(super) fn from_compiled_source(
        pat: &'a str,
        journal: &Journal,
    ) -> Result<Self, ProviderFailure> {
        let compiled_identity = crate::compile_identity::compiled_release_identity()
            .ok_or(ProviderFailure::TrustedSourcePinMissing)?;
        let source_revision = compiled_source_revision()?;
        if pat.trim().is_empty() {
            return Err(ProviderFailure::CredentialMissing);
        }
        let state_directory = journal
            .path()
            .parent()
            .ok_or(ProviderFailure::NativeAdmission)?;
        let native = NativeArtifactProvider::from_compiled_release_identity(pat, state_directory)
            .map_err(|_| ProviderFailure::NativeAdmission)?;
        let cache_key = PreparedReleaseCacheKey {
            source_revision: source_revision.clone(),
            helper_sha256: *compiled_identity.helper_sha256(),
            state_directory: state_directory.to_path_buf(),
        };
        Ok(Self {
            native: Some(native),
            source_revision,
            cache_key: Some(cache_key),
        })
    }

    #[cfg(test)]
    fn with_source_revision(pat: &'a str, source_revision: &str) -> Result<Self, ProviderFailure> {
        if pat.trim().is_empty() {
            return Err(ProviderFailure::CredentialMissing);
        }
        if !valid_sha(source_revision) {
            return Err(ProviderFailure::TrustedSourcePinInvalid);
        }
        Ok(Self {
            native: None,
            source_revision: source_revision.to_owned(),
            cache_key: None,
        })
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
        _journal: &Journal,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        std::future::ready(Ok(None)).await
    }
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
