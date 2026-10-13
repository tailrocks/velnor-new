//! Controller-only guest resource probe. Runner projections stay untouched.

use bollard::Docker;

use crate::journal::Journal;
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

use self::provider::UnavailableImageProvider;
pub(super) use self::sample::StartPermit;

mod execute;
mod inspect;
mod lifecycle;
mod ops;
mod projection;
mod provider;
mod record;
mod recover;
mod sample;

#[cfg(test)]
mod tests;

/// Obtain a fresh one-use admission permit immediately before acquire/JIT/start.
pub(super) async fn start_permit(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    test_admission: super::drive::GuestAdmission,
) -> Option<StartPermit> {
    #[cfg(test)]
    if let TestOverride::Permit(permit) = test_override(test_admission) {
        return Some(permit);
    }
    #[cfg(test)]
    if matches!(test_admission, super::drive::GuestAdmission::Unavailable) {
        return None;
    }
    #[cfg(not(test))]
    let _ = test_admission;

    Box::pin(lifecycle::collect(
        docker,
        journal,
        &UnavailableImageProvider,
    ))
    .await
    .ok()
    .flatten()
    .and_then(|sample| sample.into_start_permit(budget))
}

#[cfg(test)]
enum TestOverride {
    Permit(StartPermit),
    Deny,
}

#[cfg(test)]
fn test_override(admission: super::drive::GuestAdmission) -> TestOverride {
    match admission {
        super::drive::GuestAdmission::FreshSample => {
            TestOverride::Permit(StartPermit::test_fixture())
        }
        super::drive::GuestAdmission::Unavailable => TestOverride::Deny,
    }
}

/// Obtain a separate sample for D21 poll-header adjustment; it never authorizes a start.
#[cfg(test)]
pub(super) async fn pressure_sample(
    _docker: &Docker,
    _journal: &Journal,
) -> Option<super::pressure::Sample> {
    std::future::ready(None).await
}

/// Obtain a separate sample for D21 poll-header adjustment; it never authorizes a start.
#[cfg(not(test))]
pub(super) async fn pressure_sample(
    docker: &Docker,
    journal: &Journal,
) -> Option<super::pressure::Sample> {
    Box::pin(lifecycle::collect(
        docker,
        journal,
        &UnavailableImageProvider,
    ))
    .await
    .ok()
    .flatten()
    .and_then(sample::Observation::pressure)
}

pub(super) fn consume_permit(permit: StartPermit, engine_id: &str, root_digest: &str) -> bool {
    permit.consume(engine_id, root_digest)
}

pub(super) fn docker_root_digest(path: &str) -> Result<String, crate::error::HostError> {
    projection::DockerRoot::parse(path).map(|root| root.digest().to_owned())
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
