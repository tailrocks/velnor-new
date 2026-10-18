//! Verified resource-probe image acquisition and daemon binding.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerConfig, ImageInspect, SystemInfo};
use tokio::sync::{Mutex, OnceCell};

use crate::error::HostError;
use crate::journal::{ImageImportState, Journal};

mod archive;
mod failure;
mod gh;
mod identity;
mod release;
#[cfg(test)]
mod tests;
mod worker_import;
mod worker_prepare;
mod worker_stream;
mod worker_verify;

pub(super) use failure::ProviderFailure;
use identity::{
    SelectedDaemon, candidate, compiled_source_revision, image_import_request, inspect_image,
    selected_daemon, verified_import_inspection, verify_daemon_image, verify_selected_daemon,
};
pub(in crate::launch) use worker_import::VerifiedWorkerImageIdentityProof;

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
        journal: &Journal,
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
        journal: &Journal,
        selected: &SelectedDaemon,
        release: F,
    ) -> Result<VerifiedCandidate, ProviderFailure>
    where
        F: Future<Output = Result<Arc<release::PreparedRelease>, ProviderFailure>>,
    {
        let release = release.await?;
        verify_selected_daemon(docker, selected).await?;
        let image = self
            .inspect_or_load(docker, journal, selected, &release)
            .await?;
        candidate(&selected.engine_id, &selected.architecture, image, &release)
    }

    async fn verified_candidate_with_release<F>(
        &self,
        docker: &Docker,
        journal: &Journal,
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
            self.candidate_from_release(docker, journal, &selected, release),
        )
        .await
        .map_err(|_| ProviderFailure::Timeout)??;
        Ok(Some(candidate))
    }

    async fn inspect_or_load(
        &self,
        docker: &Docker,
        journal: &Journal,
        selected: &SelectedDaemon,
        release: &release::PreparedRelease,
    ) -> Result<ImageInspect, ProviderFailure> {
        let request = image_import_request(selected, release)?;
        let record = journal
            .prepare_image_import(request.clone())
            .await
            .map_err(|_| ProviderFailure::ImageImportJournal)?;

        match record.state {
            ImageImportState::Prepared | ImageImportState::Completed => {}
            ImageImportState::Requested | ImageImportState::Uncertain => {
                // The prior /images/load effect may still be in flight. Inspection
                // alone cannot prove its response stream drained or fence its owner.
                return Err(ProviderFailure::ImageLoad);
            }
            ImageImportState::Aborted => return Err(ProviderFailure::ImageImportJournal),
        }

        for reference in [
            release.image_reference.as_str(),
            release.config_digest.as_str(),
            release.manifest_digest.as_str(),
        ] {
            if let Some(image) = inspect_image(docker, reference).await? {
                verify_daemon_image(&image, release)?;
                let inspection =
                    verified_import_inspection(docker, selected, release, &request, &image).await?;
                if record.state == ImageImportState::Prepared {
                    journal
                        .complete_preloaded_image_import(record.id, &request, &inspection)
                        .await
                        .map_err(|_| ProviderFailure::ImageImportJournal)?;
                } else if record.image_id.as_deref() != Some(inspection.image_id()) {
                    return Err(ProviderFailure::ImageIdentity);
                } else {
                    verify_selected_daemon(docker, selected).await?;
                }
                return Ok(image);
            }
        }

        if record.state != ImageImportState::Prepared {
            return Err(ProviderFailure::ImageLoad);
        }

        verify_selected_daemon(docker, selected).await?;
        let claim = journal
            .claim_image_import_effect(record.id, &request)
            .await
            .map_err(|_| ProviderFailure::ImageImportJournal)?;
        let Some(claim) = claim else {
            return Err(ProviderFailure::ImageLoad);
        };

        let result = self.load_and_inspect(docker, selected, release).await;
        let image = match result {
            Ok(image) => image,
            Err(error) => {
                journal
                    .mark_image_import_uncertain(claim)
                    .await
                    .map_err(|_| ProviderFailure::ImageImportJournal)?;
                return Err(error);
            }
        };
        let inspection =
            match verified_import_inspection(docker, selected, release, &request, &image).await {
                Ok(inspection) => inspection,
                Err(error) => {
                    journal
                        .mark_image_import_uncertain(claim)
                        .await
                        .map_err(|_| ProviderFailure::ImageImportJournal)?;
                    return Err(error);
                }
            };
        journal
            .complete_claimed_image_import(claim, &inspection)
            .await
            .map_err(|_| ProviderFailure::ImageImportJournal)?;
        Ok(image)
    }

    async fn load_and_inspect(
        &self,
        docker: &Docker,
        selected: &SelectedDaemon,
        release: &release::PreparedRelease,
    ) -> Result<ImageInspect, ProviderFailure> {
        // The durable Requested row was committed before this selected-engine check.
        // Any mismatch after the claim remains unresolved and blocks another import.
        verify_selected_daemon(docker, selected).await?;
        if inspect_image(docker, &release.image_reference)
            .await?
            .is_some()
        {
            return Err(ProviderFailure::ImageIdentity);
        }
        verify_selected_daemon(docker, selected).await?;
        let response = docker.import_image(
            bollard::query_parameters::ImportImageOptionsBuilder::default().build(),
            bollard::body_full(release.load_archive.clone().into()),
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
        let image = inspect_image(docker, &release.image_reference)
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
        journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        let result = self
            .verified_candidate_with_release(
                docker,
                journal,
                engine_id,
                info,
                self.verified_release(),
            )
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
        _journal: &Journal,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        std::future::ready(Ok(None)).await
    }
}

type PreparedReleaseCell = Arc<OnceCell<Arc<release::PreparedRelease>>>;

static PREPARED_RELEASES: OnceLock<Mutex<HashMap<String, PreparedReleaseCell>>> = OnceLock::new();

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
