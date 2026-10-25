//! Prepare both worker images from one source-pinned, attested release.

use bollard::Docker;

use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};

use super::identity::{SelectedDaemon, verify_selected_daemon};
use super::release::PreparedWorkerArtifact;
use super::worker_import::import_worker_artifact;
use super::{PLATFORM, ProviderFailure, ReleaseImageProvider};

#[cfg(test)]
use super::worker_import::VerifiedWorkerImageIdentityProof;

pub(super) async fn prepare_worker_images(
    docker: &Docker,
    journal: &Journal,
    expected: &SelectedEngineContext,
    release_read_secret: &str,
) -> Result<VerifiedWorkerImages, HostError> {
    let provider = ReleaseImageProvider::from_compiled_source(release_read_secret, journal)
        .map_err(public_failure)?;
    let source_sha = provider.source_revision();
    if !valid_release_secret(release_read_secret) {
        return Err(public_failure(ProviderFailure::CredentialMissing));
    }
    if expected.platform() != PLATFORM
        || expected.engine_id().is_empty()
        || expected.docker_root_digest().len() != 64
        || !matches!(expected.daemon_architecture(), "aarch64" | "x86_64")
    {
        return Err(HostError::Identity);
    }

    let selected = SelectedDaemon {
        engine_id: expected.engine_id().to_owned(),
        docker_root_digest: expected.docker_root_digest().to_owned(),
        architecture: expected.daemon_architecture().to_owned(),
    };
    verify_selected_daemon(docker, &selected)
        .await
        .map_err(public_failure)?;

    let workspace = super::workspace::Workspace::create_native().map_err(public_failure)?;
    let material = provider
        .download_worker_material(workspace.assets())
        .await
        .map_err(public_failure)?;
    if material.source_sha != source_sha {
        return Err(public_failure(ProviderFailure::TrustedSourcePinInvalid));
    }
    let runner_artifact = prepare_worker_artifact(
        &workspace,
        &material.runner_path,
        &material.runner_sha256,
        source_sha,
        super::archive::worker::WorkerImage::Runner,
    )?;
    let dind_artifact = prepare_worker_artifact(
        &workspace,
        &material.dind_path,
        &material.dind_sha256,
        source_sha,
        super::archive::worker::WorkerImage::Dind,
    )?;
    verify_selected_daemon(docker, &selected)
        .await
        .map_err(public_failure)?;

    let runner = import_worker_artifact(docker, journal, &selected, source_sha, &runner_artifact)
        .await
        .map_err(public_failure)?;
    let dind = import_worker_artifact(docker, journal, &selected, source_sha, &dind_artifact)
        .await
        .map_err(public_failure)?;

    verify_selected_daemon(docker, &selected)
        .await
        .map_err(public_failure)?;
    let cleanup = workspace.remove_verified().map_err(public_failure)?;
    verify_selected_daemon(docker, &selected)
        .await
        .map_err(public_failure)?;
    VerifiedWorkerImages::from_verified_proofs(expected, source_sha, cleanup, runner, dind)
}

fn prepare_worker_artifact(
    workspace: &super::workspace::Workspace,
    archive_path: &std::path::Path,
    archive_sha256: &str,
    source_sha: &str,
    image: super::archive::worker::WorkerImage,
) -> Result<PreparedWorkerArtifact, HostError> {
    let original = super::archive::worker::verify(
        archive_path,
        image,
        source_sha,
        archive_sha256,
        &image.release_tag(),
    )
    .map_err(public_failure)?;
    let output_path = workspace
        .assets()
        .join(format!("{}-load.tar", image.image_name()));
    let load = super::archive::worker::derive_load_archive(
        archive_path,
        &output_path,
        image,
        source_sha,
        &original,
    )
    .map_err(public_failure)?;
    Ok(PreparedWorkerArtifact {
        image,
        original,
        load,
    })
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

#[cfg(test)]
pub(super) fn fixture_proofs(
    selected: &SelectedDaemon,
    source_sha: &str,
) -> (
    VerifiedWorkerImageIdentityProof,
    VerifiedWorkerImageIdentityProof,
) {
    (
        VerifiedWorkerImageIdentityProof::test_fixture(
            selected,
            source_sha,
            super::archive::worker::WorkerImage::Runner,
            1,
        ),
        VerifiedWorkerImageIdentityProof::test_fixture(
            selected,
            source_sha,
            super::archive::worker::WorkerImage::Dind,
            5,
        ),
    )
}

#[cfg(test)]
mod tests;
