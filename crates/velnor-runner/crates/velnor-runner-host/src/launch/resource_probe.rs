// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use std::sync::Arc;

use crate::journal::Journal;
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

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

    let result = Box::pin(collect(
        docker,
        journal,
        rest.images.and_then(|slot| slot.images()),
    ))
    .await;
    if result.is_err()
        && let Some(slot) = rest.images
    {
        slot.invalidate();
    }
    result
        .ok()
        .flatten()
        .and_then(|sample| sample.into_start_permit(budget))
}

pub(super) async fn pressure_sample(
    docker: &Docker,
    journal: &Journal,
    rest: Rest<'_>,
) -> Option<super::pressure::Sample> {
    let result = Box::pin(collect(
        docker,
        journal,
        rest.images.and_then(|slot| slot.images()),
    ))
    .await;
    if result.is_err()
        && let Some(slot) = rest.images
    {
        slot.invalidate();
    }
    result
        .ok()
        .flatten()
        .and_then(sample::Observation::pressure)
}

struct ArtifactImageProvider<'a> {
    images: &'a crate::artifact_admission::AdmittedImages,
}

async fn collect(
    docker: &Docker,
    journal: &Journal,
    images: Option<Arc<crate::artifact_admission::AdmittedImages>>,
) -> Result<Option<sample::Observation>, crate::error::HostError> {
    let images = images.ok_or(crate::error::HostError::Identity)?;
    Box::pin(lifecycle::collect(
        docker,
        journal,
        &ArtifactImageProvider { images: &images },
    ))
    .await
}

impl provider::ImageProvider for ArtifactImageProvider<'_> {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &bollard::models::SystemInfo,
    ) -> Result<Option<provider::VerifiedCandidate>, crate::error::HostError> {
        self.images.verify_engine(docker).await?;
        if engine_id != self.images.engine_id
            || info.id.as_deref() != Some(engine_id)
            || info.docker_root_dir.as_deref() != Some(self.images.docker_root.as_str())
        {
            return Err(crate::error::HostError::Identity);
        }
        let candidate = self.images.probe.clone();
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
