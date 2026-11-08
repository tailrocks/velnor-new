// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};

use super::identity::{SelectedDaemon, verify_selected_daemon};
use super::{PLATFORM, ProviderFailure, ReleaseImageProvider};
use crate::launch::verified_images::RegistryWorkerImage;
use velnor_runner_image_manifest::ImageKind;

pub(super) async fn prepare_worker_images(
    docker: &Docker,
    journal: &Journal,
    expected: &SelectedEngineContext,
    release_read_secret: &str,
) -> Result<VerifiedWorkerImages, HostError> {
    let (provider, selected) = setup(journal, expected, release_read_secret)?;
    let native = provider.native.as_ref().ok_or(HostError::Identity)?;
    let release = native.verified_runner_release().await?;
    verify_selected_daemon(docker, &selected)
        .await
        .map_err(public_failure)?;
    let selection = super::DaemonSelection::new(
        &selected.engine_id,
        &selected.docker_root_digest,
        &selected.architecture,
    )?;
    let runner_spec = super::registry_spec(native, &release, ImageKind::Runner, &selection)
        .await
        .map_err(public_failure)?;
    let runner = super::acquire_registry_image(journal, expected.endpoint(), runner_spec).await?;
    let dind_spec = super::registry_spec(native, &release, ImageKind::Dind, &selection)
        .await
        .map_err(public_failure)?;
    let dind = super::acquire_registry_image(journal, expected.endpoint(), dind_spec).await?;
    VerifiedWorkerImages::from_registry(
        expected,
        &release.source_sha,
        &release.manifest_sha,
        worker_image(runner),
        worker_image(dind),
    )
}

fn setup<'a>(
    journal: &Journal,
    expected: &SelectedEngineContext,
    secret: &'a str,
) -> Result<(ReleaseImageProvider<'a>, SelectedDaemon), HostError> {
    if !valid_release_secret(secret) {
        return Err(public_failure(ProviderFailure::CredentialMissing));
    }
    if expected.platform() != PLATFORM
        || expected.engine_id().is_empty()
        || expected.docker_root_digest().len() != 64
        || !matches!(expected.daemon_architecture(), "aarch64" | "x86_64")
    {
        return Err(HostError::Identity);
    }
    let provider = ReleaseImageProvider::from_compiled_source(secret, journal, expected.endpoint())
        .map_err(public_failure)?;
    let selected = SelectedDaemon {
        engine_id: expected.engine_id().to_owned(),
        docker_root_digest: expected.docker_root_digest().to_owned(),
        architecture: expected.daemon_architecture().to_owned(),
    };
    Ok((provider, selected))
}

fn worker_image(image: super::registry_import::AcquiredRegistryImage) -> RegistryWorkerImage {
    RegistryWorkerImage {
        identity: image.identity,
        observation: image.observation,
        projection: image.projection,
    }
}

fn public_failure(error: ProviderFailure) -> HostError {
    eprintln!("worker_images=unavailable reason={}", error.code());
    HostError::Docker
}

fn valid_release_secret(secret: &str) -> bool {
    !secret.is_empty()
        && secret
            .bytes()
            .all(|byte| !byte.is_ascii_whitespace() && !byte.is_ascii_control())
}
