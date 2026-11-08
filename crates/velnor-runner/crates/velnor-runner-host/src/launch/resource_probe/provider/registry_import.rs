// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::ImageInspect;
use velnor_runner_image_manifest::ImageKind;

use super::ProviderFailure;
use super::identity::SelectedDaemon;
use super::image_acquisition::{
    DaemonSelection, ImageObservation, ImageReference, ImageSession, ImageStore,
};
use crate::artifact_admission::{
    NativeArtifactProvider, RegistryProjection, VerifiedRunnerRelease,
};
use crate::journal::{
    ImageImportState, Journal, RegistryImageIdentity, RegistryImageInspection,
    RegistryImageObservation,
};

pub(super) struct AdmittedRegistryImage {
    pub(super) identity: RegistryImageIdentity,
    pub(super) observation: RegistryImageObservation,
    pub(super) configuration: bollard::models::ContainerConfig,
    pub(super) projection: RegistryProjection,
}

pub(super) async fn reinspect_bound(
    endpoint: &str,
    selected: &SelectedDaemon,
    identity: &RegistryImageIdentity,
    projection: &RegistryProjection,
) -> Result<RegistryImageObservation, ProviderFailure> {
    let mut session = image_session(endpoint, selected).await?;
    let reference = image_reference(identity)?;
    let image = session
        .inspect(&reference)
        .await
        .map_err(|_| ProviderFailure::ImageInspect)?
        .ok_or(ProviderFailure::ImageIdentity)?;
    let (inspection, _) = verified_inspection(identity, projection, image)?;
    Ok(inspection.observation().clone())
}

async fn image_session(
    endpoint: &str,
    selected: &SelectedDaemon,
) -> Result<ImageSession, ProviderFailure> {
    let expected = DaemonSelection::new(
        &selected.engine_id,
        &selected.docker_root_digest,
        &selected.architecture,
    )
    .map_err(|_| ProviderFailure::DaemonIdentity)?;
    ImageSession::connect(endpoint, &expected)
        .await
        .map_err(|_| ProviderFailure::DaemonIdentity)
}

fn image_reference(identity: &RegistryImageIdentity) -> Result<ImageReference, ProviderFailure> {
    ImageReference::new(identity.role, &identity.registry_digest)
        .map_err(|_| ProviderFailure::ImageIdentity)
}

pub(super) async fn admit(
    provider: &NativeArtifactProvider<'_>,
    release: &VerifiedRunnerRelease,
    role: ImageKind,
    journal: &Journal,
    selected: &SelectedDaemon,
    endpoint: &str,
) -> Result<AdmittedRegistryImage, ProviderFailure> {
    let (identity, projection) = registry_identity(provider, release, role, selected).await?;
    let mut session = image_session(endpoint, selected).await?;
    let reference = image_reference(&identity)?;
    let record = journal
        .prepare_registry_image(identity.clone())
        .await
        .map_err(|_| ProviderFailure::ImageImportJournal)?;
    if matches!(
        record.state,
        ImageImportState::Requested | ImageImportState::Uncertain | ImageImportState::Aborted
    ) {
        return Err(ProviderFailure::ImageLoad);
    }
    if let Some(image_inspect) = session
        .inspect(&reference)
        .await
        .map_err(|_| ProviderFailure::ImageInspect)?
    {
        let (inspection, configuration) =
            verified_inspection(&identity, &projection, image_inspect)?;
        match record.state {
            ImageImportState::Prepared => journal
                .complete_preloaded_registry_image(&record, &inspection)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?,
            ImageImportState::Completed => journal
                .verify_completed_registry_image(&record, &inspection)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?,
            _ => return Err(ProviderFailure::ImageLoad),
        }
        return Ok(AdmittedRegistryImage {
            identity,
            observation: inspection.observation().clone(),
            configuration,
            projection,
        });
    }
    if record.state != ImageImportState::Prepared {
        return Err(ProviderFailure::ImageLoad);
    }
    let claim = journal
        .claim_registry_image(&record)
        .await
        .map_err(|_| ProviderFailure::ImageImportJournal)?
        .ok_or(ProviderFailure::ImageImportJournal)?;
    let result = match session.pull_and_inspect(&reference).await {
        Ok(observation) => verified_inspection(&identity, &projection, observation),
        Err(_) => Err(ProviderFailure::ImageLoad),
    };
    match result {
        Ok((inspection, configuration)) => {
            journal
                .complete_registry_image(claim, &inspection)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?;
            Ok(AdmittedRegistryImage {
                identity,
                observation: inspection.observation().clone(),
                configuration,
                projection,
            })
        }
        Err(error) => {
            journal
                .mark_registry_image_uncertain(claim)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?;
            Err(error)
        }
    }
}
async fn registry_identity(
    provider: &NativeArtifactProvider<'_>,
    release: &VerifiedRunnerRelease,
    role: ImageKind,
    selected: &SelectedDaemon,
) -> Result<(RegistryImageIdentity, RegistryProjection), ProviderFailure> {
    let image = match role {
        ImageKind::Runner => &release.manifest.images.runner,
        ImageKind::Dind => &release.manifest.images.dind,
        ImageKind::ResourceProbe => &release.manifest.images.resource_probe,
    };
    let projection = provider
        .verified_registry_projection(release, role)
        .await
        .map_err(|_| ProviderFailure::NativeAdmission)?;
    let identity = RegistryImageIdentity::new(
        &selected.engine_id,
        &selected.docker_root_digest,
        &selected.architecture,
        &release.source_sha,
        &release.manifest_sha,
        role,
        image,
    )
    .map_err(|_| ProviderFailure::ImageIdentity)?;
    Ok((identity, projection))
}
fn verified_inspection(
    identity: &RegistryImageIdentity,
    projection: &RegistryProjection,
    observation: ImageObservation,
) -> Result<(RegistryImageInspection, bollard::models::ContainerConfig), ProviderFailure> {
    if !projection.matches(
        identity.role,
        &identity.source_sha,
        &identity.registry_digest,
        &identity.config_digest,
    ) {
        return Err(ProviderFailure::ImageIdentity);
    }
    let ImageObservation {
        inspect,
        store,
        live,
    } = observation;
    if inspect.os.as_deref() != Some("linux")
        || inspect.architecture.as_deref() != Some("amd64")
        || inspect
            .os_version
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || inspect
            .variant
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || !inspect
            .repo_digests
            .as_ref()
            .is_some_and(|digests| digests.iter().any(|value| value == &identity.reference()))
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    let runtime_id = inspect.id.as_ref().ok_or(ProviderFailure::ImageIdentity)?;
    let observed = match inspect.descriptor.as_ref() {
        None if store == ImageStore::Classic && runtime_id == &identity.config_digest => {
            RegistryImageObservation::Classic {
                runtime_id: runtime_id.clone(),
                manifest_size: projection.manifest_size(),
            }
        }
        Some(descriptor)
            if store == ImageStore::Containerd
                && runtime_id == &identity.registry_digest
                && descriptor.digest.as_deref() == Some(identity.registry_digest.as_str())
                && descriptor.media_type.as_deref()
                    == Some("application/vnd.oci.image.manifest.v1+json")
                && descriptor.size.and_then(|value| u64::try_from(value).ok())
                    == Some(projection.manifest_size())
                && descriptor.platform.as_ref().is_none_or(|platform| {
                    platform.os.as_deref() == Some("linux")
                        && platform.architecture.as_deref() == Some("amd64")
                        && platform.variant.as_deref().is_none_or(str::is_empty)
                        && platform.os_version.as_deref().is_none_or(str::is_empty)
                        && platform.os_features.as_ref().is_none_or(Vec::is_empty)
                }) =>
        {
            RegistryImageObservation::Containerd {
                runtime_id: runtime_id.clone(),
                manifest_size: projection.manifest_size(),
            }
        }
        _ => return Err(ProviderFailure::ImageIdentity),
    };
    let rootfs = inspect
        .root_fs
        .as_ref()
        .ok_or(ProviderFailure::ImageIdentity)?;
    if rootfs.typ != "layers" || rootfs.layers.as_deref() != Some(projection.diff_ids()) {
        return Err(ProviderFailure::ImageIdentity);
    }
    let configuration = verify_configuration(&inspect, identity, projection)?;
    let inspection = RegistryImageInspection::verified(
        identity,
        &live.engine_id,
        &live.docker_root_digest,
        &live.architecture,
        observed,
    )
    .map_err(|_| ProviderFailure::ImageIdentity)?;
    Ok((inspection, configuration))
}

fn verify_configuration(
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
        ImageKind::Runner => Some(super::WorkerImage::Runner),
        ImageKind::Dind => Some(super::WorkerImage::Dind),
        ImageKind::ResourceProbe => None,
    } {
        super::verify_worker_runtime_config(&config_value, worker, &identity.source_sha)
            .map_err(|_| ProviderFailure::ImageConfig)?;
    }
    let configuration =
        serde_json::from_value(config_value).map_err(|_| ProviderFailure::ImageConfig)?;
    Ok(configuration)
}
