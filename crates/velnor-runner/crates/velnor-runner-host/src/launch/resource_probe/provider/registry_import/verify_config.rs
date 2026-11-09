// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ProviderFailure, RegistryImageIdentity, RegistryProjection};
use bollard::models::ImageInspect;
use velnor_runner_image_manifest::ImageKind;

pub(super) fn verify_configuration(
    inspect: &ImageInspect,
    identity: &RegistryImageIdentity,
    projection: &RegistryProjection,
) -> Result<bollard::models::ContainerConfig, ProviderFailure> {
    let actual = inspect
        .config
        .as_ref()
        .ok_or(ProviderFailure::ImageConfig)?;
    let expected: bollard::models::ImageConfig =
        serde_json::from_value(projection.configuration().clone())
            .map_err(|_| ProviderFailure::ImageConfig)?;
    let expected_health =
        crate::resource_probe_release::runner_qualification::image_identity::source_healthcheck(
            projection.configuration(),
        )
        .map_err(|_| ProviderFailure::ImageConfig)?;
    if actual.labels != expected.labels
        || actual.user != expected.user
        || actual.working_dir != expected.working_dir
        || actual.entrypoint != expected.entrypoint
        || actual.cmd != expected.cmd
        || actual.env != expected.env
        || actual
            .volumes
            .as_ref()
            .is_some_and(|volumes| !volumes.is_empty())
        || !crate::resource_probe_release::runner_qualification::image_identity::same_healthcheck(
            actual.healthcheck.as_ref(),
            expected_health.as_ref(),
        )
    {
        return Err(ProviderFailure::ImageConfig);
    }
    let config_value = serde_json::to_value(actual).map_err(|_| ProviderFailure::ImageConfig)?;
    if let Some(worker) = match identity.role {
        ImageKind::Runner => Some(super::super::WorkerImage::Runner),
        ImageKind::Dind => Some(super::super::WorkerImage::Dind),
        ImageKind::ResourceProbe => None,
    } {
        super::super::verify_worker_runtime_config(&config_value, worker, &identity.source_sha)
            .map_err(|_| ProviderFailure::ImageConfig)?;
    }
    let configuration =
        serde_json::from_value(config_value).map_err(|_| ProviderFailure::ImageConfig)?;
    Ok(configuration)
}
