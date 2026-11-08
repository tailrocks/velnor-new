// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::ImageInspect;
use velnor_runner_image_manifest::{ImageEntry, ImageKind};

use super::ProviderFailure;
use super::identity::SelectedDaemon;
use super::image_acquisition::{
    DaemonSelection, ImageObservation, ImageReference, ImageSession, ImageStore,
};
use crate::artifact_admission::RegistryProjection;
use crate::journal::{
    ImageImportState, Journal, RegistryImageIdentity, RegistryImageInspection,
    RegistryImageObservation, RegistryImageRecord,
};

#[derive(Debug)]
pub struct AcquiredRegistryImage {
    pub(crate) identity: RegistryImageIdentity,
    pub(crate) observation: RegistryImageObservation,
    pub(crate) configuration: bollard::models::ContainerConfig,
    pub(crate) projection: RegistryProjection,
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

impl AcquiredRegistryImage {
    #[must_use]
    pub fn runtime_id(&self) -> &str {
        self.observation.runtime_id()
    }
}

#[derive(Debug)]
pub struct RegistryImageSpecification {
    identity: RegistryImageIdentity,
    projection: RegistryProjection,
}

impl RegistryImageSpecification {
    pub fn new(
        selected: &DaemonSelection,
        image: &ImageEntry,
        role: ImageKind,
        source: &str,
        manifest_sha: &str,
        projection: RegistryProjection,
    ) -> Result<Self, crate::error::HostError> {
        let identity = RegistryImageIdentity::new(
            &selected.engine_id,
            &selected.docker_root_digest,
            &selected.architecture,
            source,
            manifest_sha,
            role,
            image,
        )?;
        if !projection.matches(
            role,
            source,
            &identity.registry_digest,
            &identity.config_digest,
        ) {
            return Err(crate::error::HostError::Identity);
        }
        Ok(Self {
            identity,
            projection,
        })
    }
}

enum Prepared {
    Existing(Box<AcquiredRegistryImage>),
    Missing(Box<(ImageSession, RegistryImageRecord, ImageReference)>),
}

pub async fn acquire_registry_image(
    journal: &Journal,
    endpoint: &str,
    spec: RegistryImageSpecification,
) -> Result<AcquiredRegistryImage, crate::error::HostError> {
    acquire(journal, endpoint, spec).await.map_err(|error| {
        eprintln!("registry_image=unavailable reason={}", error.code());
        crate::error::HostError::Docker
    })
}

async fn acquire(
    journal: &Journal,
    endpoint: &str,
    spec: RegistryImageSpecification,
) -> Result<AcquiredRegistryImage, ProviderFailure> {
    let RegistryImageSpecification {
        identity,
        projection,
    } = spec;
    let selected = DaemonSelection::new(
        &identity.engine_id,
        &identity.docker_root_digest,
        &identity.daemon_architecture,
    )
    .map_err(|_| ProviderFailure::DaemonIdentity)?;
    let prepared = tokio::time::timeout(super::PREPARE_TIMEOUT, async {
        let mut session = ImageSession::connect(endpoint, &selected)
            .await
            .map_err(|_| ProviderFailure::DaemonIdentity)?;
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
            return Ok(Prepared::Existing(Box::new(AcquiredRegistryImage {
                identity: identity.clone(),
                observation: inspection.observation().clone(),
                configuration,
                projection: projection.clone(),
            })));
        }
        if record.state != ImageImportState::Prepared {
            return Err(ProviderFailure::ImageLoad);
        }
        Ok(Prepared::Missing(Box::new((session, record, reference))))
    })
    .await
    .map_err(|_| ProviderFailure::Timeout)??;
    let (mut session, record, reference) = match prepared {
        Prepared::Existing(image) => return Ok(*image),
        Prepared::Missing(missing) => *missing,
    };
    settle_import(
        journal,
        &mut session,
        &record,
        &reference,
        identity,
        projection,
    )
    .await
}

async fn settle_import(
    journal: &Journal,
    session: &mut ImageSession,
    record: &RegistryImageRecord,
    reference: &ImageReference,
    identity: RegistryImageIdentity,
    projection: RegistryProjection,
) -> Result<AcquiredRegistryImage, ProviderFailure> {
    let claim = journal
        .claim_registry_image(record)
        .await
        .map_err(|_| ProviderFailure::ImageImportJournal)?
        .ok_or(ProviderFailure::ImageImportJournal)?;
    let result = match session.pull_and_inspect(reference).await {
        Ok(observation) => verified_inspection(&identity, &projection, observation),
        Err(_) => Err(ProviderFailure::ImageLoad),
    };
    match result {
        Ok((inspection, configuration)) => {
            journal
                .complete_registry_image(claim, &inspection)
                .await
                .map_err(|_| ProviderFailure::ImageImportJournal)?;
            Ok(AcquiredRegistryImage {
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
