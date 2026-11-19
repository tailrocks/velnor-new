// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;
use bollard::models::{ContainerConfig, ImageInspect, SystemInfo};
use sha2::{Digest, Sha256};

use crate::config::{DeveloperImages, RunnerPlatform};
use crate::docker_client::docker_deadline;
use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::verified_images::DeveloperWorkerImages;
use crate::launch::{SelectedEngineContext, VerifiedImageIdentity, VerifiedWorkerImages};

use super::evidence::ProbeImageOrigin;
use super::{
    ImageProvider, ProviderFailure, VerifiedCandidate, WorkerImage, selected_daemon, valid_sha,
    verify_worker_runtime_config,
};

const SOURCE_LABEL: &str = "org.opencontainers.image.revision";

pub(in crate::launch::resource_probe) struct DeveloperImageProvider {
    probe: String,
}

impl DeveloperImageProvider {
    pub(in crate::launch::resource_probe) fn new(images: &DeveloperImages) -> Self {
        Self {
            probe: images.resource_probe().to_owned(),
        }
    }
}

impl ImageProvider for DeveloperImageProvider {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        _journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        let selected = selected_daemon(info, engine_id).map_err(|_| HostError::Docker)?;
        let platform = docker_deadline(docker.inspect_image(&self.probe))
            .await?
            .map_err(|_| HostError::Docker)?
            .architecture
            .as_deref()
            .and_then(RunnerPlatform::from_image_architecture)
            .ok_or(HostError::Identity)?;
        let inspect = inspect(docker, &self.probe, platform).await?;
        let source_revision = source_label(&inspect)?;
        let config: ContainerConfig = serde_json::from_value(
            serde_json::to_value(inspect.config.as_ref().ok_or(HostError::Docker)?)
                .map_err(|_| HostError::Docker)?,
        )
        .map_err(|_| HostError::Docker)?;
        let mut hash = Sha256::new();
        hash.update(b"velnor-resource-probe-developer-binding-v1\0");
        for value in [
            selected.engine_id.as_str(),
            selected.docker_root_digest.as_str(),
            selected.architecture.as_str(),
            platform.as_str(),
            source_revision.as_str(),
            self.probe.as_str(),
        ] {
            hash.update(value.as_bytes());
            hash.update([0]);
        }
        Ok(Some(VerifiedCandidate {
            runtime_id: self.probe.clone(),
            platform: platform.as_str().to_owned(),
            daemon_architecture: selected.architecture,
            source_revision,
            binding_fingerprint: super::hex(&hash.finalize()),
            config,
            engine_id: selected.engine_id,
            docker_root_digest: selected.docker_root_digest,
            origin: ProbeImageOrigin::Developer,
        }))
    }
}

pub(in crate::launch::resource_probe) async fn prepare_worker_images(
    docker: &Docker,
    expected: &SelectedEngineContext,
    images: &DeveloperImages,
) -> Result<VerifiedWorkerImages, HostError> {
    let platform = RunnerPlatform::parse(expected.platform()).ok_or(HostError::Identity)?;
    let runner_source =
        worker_image(docker, images.runner(), platform, WorkerImage::Runner).await?;
    let dind_source = worker_image(docker, images.dind(), platform, WorkerImage::Dind).await?;
    if runner_source != dind_source {
        return Err(HostError::Identity);
    }
    VerifiedWorkerImages::from_developer(
        expected,
        DeveloperWorkerImages {
            source_sha: runner_source,
            runner: images.runner().to_owned(),
            dind: images.dind().to_owned(),
        },
    )
}

pub(super) async fn verify_worker_image(
    docker: &Docker,
    images: &VerifiedWorkerImages,
    identity: &VerifiedImageIdentity,
    role: WorkerImage,
) -> Result<(), ProviderFailure> {
    let platform =
        RunnerPlatform::parse(images.platform()).ok_or(ProviderFailure::ImageIdentity)?;
    let source = worker_image(docker, identity.runtime_id(), platform, role)
        .await
        .map_err(|_| ProviderFailure::ImageIdentity)?;
    if source == images.source_sha() {
        Ok(())
    } else {
        Err(ProviderFailure::ImageIdentity)
    }
}

async fn worker_image(
    docker: &Docker,
    image_id: &str,
    platform: RunnerPlatform,
    role: WorkerImage,
) -> Result<String, HostError> {
    let inspect = inspect(docker, image_id, platform).await?;
    let source = source_label(&inspect)?;
    let config = serde_json::to_value(inspect.config.as_ref().ok_or(HostError::Docker)?)
        .map_err(|_| HostError::Docker)?;
    verify_worker_runtime_config(&config, role, &source)?;
    Ok(source)
}

async fn inspect(
    docker: &Docker,
    image_id: &str,
    platform: RunnerPlatform,
) -> Result<ImageInspect, HostError> {
    let inspect = docker_deadline(docker.inspect_image(image_id))
        .await?
        .map_err(|_| HostError::Docker)?;
    if inspect.id.as_deref() != Some(image_id)
        || inspect.os.as_deref() != Some("linux")
        || inspect.architecture.as_deref() != Some(platform.image_architecture())
    {
        return Err(HostError::Identity);
    }
    Ok(inspect)
}

fn source_label(inspect: &ImageInspect) -> Result<String, HostError> {
    let source = inspect
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(SOURCE_LABEL))
        .ok_or(HostError::Identity)?;
    if valid_sha(source) {
        Ok(source.clone())
    } else {
        Err(HostError::Identity)
    }
}

#[cfg(test)]
mod tests;
