//! Prepare both worker images from one source-pinned, attested release.

use bollard::Docker;

use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};

use super::identity::{SelectedDaemon, compiled_source_revision, verify_selected_daemon};
use super::release;
use super::worker_import::import_worker_artifact;
use super::{PLATFORM, ProviderFailure};

#[cfg(test)]
use super::worker_import::VerifiedWorkerImageIdentityProof;

pub(super) async fn prepare_worker_images(
    docker: &Docker,
    journal: &Journal,
    expected: &SelectedEngineContext,
    release_read_secret: &str,
) -> Result<VerifiedWorkerImages, HostError> {
    let source_sha = compiled_source_revision().map_err(public_failure)?;
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

    let release::PreparedWorkerReleases {
        workspace,
        runner: runner_artifact,
        dind: dind_artifact,
    } = release::prepare_worker_releases(release_read_secret, source_sha)
        .await
        .map_err(public_failure)?;
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
