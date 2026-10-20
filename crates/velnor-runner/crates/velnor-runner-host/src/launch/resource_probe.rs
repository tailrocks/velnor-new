//! Controller-only guest resource probe. Runner projections stay untouched.

use bollard::Docker;
use std::path::Path;

use crate::journal::Journal;
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

use self::provider::UnavailableImageProvider;
pub(super) use self::sample::StartPermit;
use super::Rest;

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
    rest: Rest<'_>,
) -> Option<StartPermit> {
    #[cfg(test)]
    if let TestOverride::Permit(permit) = test_override(rest.guest_admission) {
        return Some(permit);
    }
    #[cfg(test)]
    if matches!(
        rest.guest_admission,
        super::drive::GuestAdmission::Unavailable
    ) {
        return None;
    }
    #[cfg(not(test))]
    let _ = rest.guest_admission;

    Box::pin(collect(docker, journal, rest.pat))
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
    _rest: Rest<'_>,
) -> Option<super::pressure::Sample> {
    std::future::ready(None).await
}

/// Obtain a separate sample for D21 poll-header adjustment; it never authorizes a start.
#[cfg(not(test))]
pub(super) async fn pressure_sample(
    docker: &Docker,
    journal: &Journal,
    rest: Rest<'_>,
) -> Option<super::pressure::Sample> {
    Box::pin(collect(docker, journal, rest.pat))
        .await
        .ok()
        .flatten()
        .and_then(sample::Observation::pressure)
}

struct ArtifactImageProvider<'a> {
    inner: crate::artifact_admission::NativeArtifactProvider<'a>,
}

impl<'a> ArtifactImageProvider<'a> {
    fn from_rest(pat: &'a str, state_directory: &Path) -> Result<Self, crate::error::HostError> {
        crate::artifact_admission::NativeArtifactProvider::from_compiled_release_identity(
            pat,
            state_directory,
        )
        .map(|inner| Self { inner })
    }
}

async fn collect(
    docker: &Docker,
    journal: &Journal,
    pat: &str,
) -> Result<Option<sample::Observation>, crate::error::HostError> {
    let state_directory = journal.state_directory()?;
    let provider = provider_for_build(
        pat,
        state_directory,
        crate::compile_identity::compiled_release_identity().is_some(),
    )?;
    let Some(provider) = provider else {
        return Box::pin(lifecycle::collect(
            docker,
            journal,
            &UnavailableImageProvider,
        ))
        .await;
    };
    Box::pin(lifecycle::collect(docker, journal, &provider)).await
}

fn provider_for_build<'a>(
    pat: &'a str,
    state_directory: &Path,
    has_compiled_identity: bool,
) -> Result<Option<ArtifactImageProvider<'a>>, crate::error::HostError> {
    if has_compiled_identity {
        ArtifactImageProvider::from_rest(pat, state_directory).map(Some)
    } else {
        Ok(None)
    }
}

impl provider::ImageProvider for ArtifactImageProvider<'_> {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &bollard::models::SystemInfo,
    ) -> Result<Option<provider::VerifiedCandidate>, crate::error::HostError> {
        let Some(candidate) = self
            .inner
            .verified_candidate(docker, engine_id, info)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(provider::VerifiedCandidate {
            runtime_id: candidate.runtime_id,
            platform: candidate.platform,
            source_revision: candidate.source_revision,
            binding_fingerprint: candidate.binding_fingerprint,
            config: candidate.config,
        }))
    }
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

#[cfg(test)]
mod provider_selection_tests {
    use super::provider_for_build;
    use crate::error::HostError;
    use std::path::Path;

    #[test]
    fn missing_compiled_identity_selects_only_the_unavailable_provider() -> Result<(), HostError> {
        if provider_for_build("local-only", Path::new("/tmp/velnor-state"), false)?.is_some() {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    #[test]
    fn provider_factory_errors_are_not_converted_to_unavailable() {
        assert_eq!(
            provider_for_build("", Path::new("/tmp/velnor-state"), true).err(),
            Some(HostError::Identity)
        );
    }
}
