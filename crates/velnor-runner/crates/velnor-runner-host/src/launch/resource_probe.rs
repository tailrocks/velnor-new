// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(super) async fn start_permit(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    rest: Rest<'_>,
) -> Option<StartPermit> {
    let _ = rest.guest_admission;

    Box::pin(collect(docker, journal, rest.pat))
        .await
        .ok()
        .flatten()
        .and_then(|sample| sample.into_start_permit(budget))
}

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
