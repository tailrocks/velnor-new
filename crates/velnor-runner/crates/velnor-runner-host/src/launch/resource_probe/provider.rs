// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use bollard::models::SystemInfo;

use crate::artifact_admission::NativeArtifactProvider;
use crate::error::HostError;
use crate::journal::Journal;

mod archive;
pub(crate) use archive::worker::{ProjectedCandidate, inspect_projected_candidate};
pub use archive::worker::{
    WorkerImage, verify_worker_attestation_manifest, verify_worker_build_source,
    verify_worker_runtime_config,
};
mod evidence;
mod failure;
mod identity;
mod image_acquisition;
pub use image_acquisition::{DaemonSelection, ImageObservation, ImageReference, ImageSession};
mod registry_import;
pub use crate::artifact_admission::RegistryProjection;
pub use registry_import::{
    AcquiredRegistryImage, RegistryImageSpecification, acquire_registry_image,
};
mod release_provider;

mod worker_prepare;
mod worker_verify;

pub(crate) use evidence::VerifiedCandidate;
pub(in crate::launch) use evidence::{ProbeImageOrigin, ProbeImagePreparationEvidence};
pub(super) use failure::ProviderFailure;
use identity::{compiled_source_revision, selected_daemon};

const PLATFORM: &str = "linux/amd64";
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

pub(crate) trait ImageProvider {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError>;
}

pub(super) struct ReleaseImageProvider<'a> {
    native: Option<NativeArtifactProvider<'a>>,
    endpoint: String,
}

impl<'a> ReleaseImageProvider<'a> {
    pub(super) fn from_compiled_source(
        pat: &'a str,
        journal: &Journal,
        endpoint: &str,
    ) -> Result<Self, ProviderFailure> {
        crate::compile_identity::compiled_release_identity()
            .ok_or(ProviderFailure::TrustedSourcePinMissing)?;
        compiled_source_revision()?;
        if pat.trim().is_empty() {
            return Err(ProviderFailure::CredentialMissing);
        }
        let state_directory = journal
            .path()
            .parent()
            .ok_or(ProviderFailure::NativeAdmission)?;
        let native = NativeArtifactProvider::from_compiled_release_identity(pat, state_directory)
            .map_err(|_| ProviderFailure::NativeAdmission)?;
        Ok(Self {
            native: Some(native),
            endpoint: endpoint.to_owned(),
        })
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

async fn registry_spec(
    native: &NativeArtifactProvider<'_>,
    release: &crate::artifact_admission::VerifiedRunnerRelease,
    role: velnor_runner_image_manifest::ImageKind,
    selected: &DaemonSelection,
) -> Result<RegistryImageSpecification, ProviderFailure> {
    use velnor_runner_image_manifest::ImageKind;
    let image = match role {
        ImageKind::Runner => &release.manifest.images.runner,
        ImageKind::Dind => &release.manifest.images.dind,
        ImageKind::ResourceProbe => &release.manifest.images.resource_probe,
    };
    let projection = native
        .verified_registry_projection(role)
        .await
        .map_err(|_| ProviderFailure::NativeAdmission)?;
    RegistryImageSpecification::new(
        selected,
        image,
        role,
        &release.source_sha,
        &release.manifest_sha,
        projection,
    )
    .map_err(|_| ProviderFailure::ImageIdentity)
}
