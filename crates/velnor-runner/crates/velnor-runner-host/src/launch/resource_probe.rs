//! Controller-only guest resource probe. Runner projections stay untouched.

use bollard::{Docker, models::SystemInfo};

use crate::journal::Journal;
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

use self::provider::ReleaseImageProvider;
pub(crate) use self::sample::StartPermit;

mod execute;
mod inspect;
mod lifecycle;
mod ops;
mod projection;
mod provider;
mod record;
mod recover;
mod sample;

/// Report only whether this binary has a valid trusted release source pin.
/// This does not claim that GitHub, the release, or any Docker image is ready.
pub(crate) fn build_pin_status() -> Result<(), &'static str> {
    provider::build_pin_status()
}

/// Re-read the selected engine and inspect both prepared worker images.
pub(crate) async fn verify_worker_images_for_engine(
    docker: &Docker,
    expected: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<(), crate::error::HostError> {
    provider::verify_worker_images_for_engine(docker, expected, images).await
}

#[cfg(test)]
mod tests;

/// Obtain a fresh one-use admission permit immediately before acquire/JIT/start.
pub(super) async fn start_permit(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    pat: &str,
) -> Option<StartPermit> {
    let provider = match ReleaseImageProvider::from_compiled_source(pat) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("resource_probe=unavailable reason={}", error.code());
            return None;
        }
    };
    match Box::pin(lifecycle::collect(docker, journal, &provider)).await {
        Ok(Some(sample)) => sample.into_start_permit(budget),
        Ok(None) | Err(_) => None,
    }
}

/// Obtain a fresh selected-guest sample for D21 poll-header adjustment.
///
/// This sample never authorizes a start.
pub(super) async fn pressure_sample(
    docker: &Docker,
    journal: &Journal,
    pat: &str,
) -> Option<super::pressure::Sample> {
    let provider = match ReleaseImageProvider::from_compiled_source(pat) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!(
                "resource_probe=pressure_unavailable reason={}",
                error.code()
            );
            return None;
        }
    };
    match Box::pin(lifecycle::collect(docker, journal, &provider)).await {
        Ok(Some(observation)) => observation.pressure(),
        Ok(None) | Err(_) => None,
    }
}

pub(super) fn consume_permit(permit: StartPermit, engine_id: &str, root_digest: &str) -> bool {
    permit.consume(engine_id, root_digest)
}

pub(super) fn docker_root_digest(path: &str) -> Result<String, crate::error::HostError> {
    projection::DockerRoot::parse(path).map(|root| root.digest().to_owned())
}

pub(super) fn daemon_architecture(info: &SystemInfo) -> Option<&'static str> {
    provider::daemon_architecture(info)
}

pub(super) fn daemon_platform_matches(info: &SystemInfo) -> bool {
    provider::daemon_platform_matches(info)
}

pub(super) async fn engine_binding(docker: &Docker) -> Result<(String, String), EnsureError> {
    let info = crate::docker_client::docker_deadline(docker.info())
        .await
        .map_err(|_| docker_identity_error())?
        .map_err(|_| docker_identity_error())?;
    let engine_id = info
        .id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(docker_identity_error)?;
    let docker_root = info
        .docker_root_dir
        .as_deref()
        .ok_or_else(docker_identity_error)?;
    let root_digest = docker_root_digest(docker_root).map_err(|_| docker_identity_error())?;
    Ok((engine_id, root_digest))
}

fn docker_identity_error() -> EnsureError {
    EnsureError::Unexpected {
        status: 0,
        step: "docker identity",
    }
}
