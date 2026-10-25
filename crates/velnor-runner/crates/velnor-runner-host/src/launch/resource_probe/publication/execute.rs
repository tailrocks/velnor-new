//! Run and remove one fixed control-file publisher.

use std::time::Instant;

use bollard::Docker;
use velnor_resource_probe::{
    execution_publication::ExecutionPublicationRecord,
    job_facts_wire::{JobFactsPublicationContext, JobFactsPublicationRecord},
};

use crate::{
    error::HostError,
    journal::{ExecutionPublicationSeed, Journal, VerifiedMetadataInitialization},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        resource_probe::{VerifiedPublicationProbeImage, projection::DockerRoot},
    },
    worker::execution_authorization::VerifiedMetadataVolume,
};

use super::{
    PublicationProjection, PublicationRecoveryProjection, VerifiedExecutionPublicationCleanup,
    request::{PublicationRecord, PublicationRequest},
};

pub(super) mod inspect;
mod run;
pub(super) mod transport;
use transport::bounded;

#[derive(Clone, Copy)]
pub(super) struct PublisherExecution<'a> {
    pub(super) docker: &'a Docker,
    pub(super) journal: &'a Journal,
    pub(super) selected: &'a SelectedEngineContext,
    pub(super) images: &'a VerifiedWorkerImages,
    pub(super) probe: &'a VerifiedPublicationProbeImage,
    pub(super) metadata: &'a VerifiedMetadataInitialization,
    pub(super) metadata_volume: &'a VerifiedMetadataVolume,
    pub(super) deadline: Instant,
}

/// Run the measured publisher and retain its record only after exact cleanup.
pub(super) async fn publish(
    execution: PublisherExecution<'_>,
    request: velnor_resource_probe::execution_publication::ExecutionPublicationContext,
    parent: crate::journal::ExecutionPublicationParent,
) -> Result<ExecutionPublicationRecord, HostError> {
    match Box::pin(publish_request(
        execution,
        PublicationRequest::Control(request),
        parent,
    ))
    .await?
    {
        PublicationRecord::Control(value) => Ok(*value),
        PublicationRecord::JobFacts(_) => Err(HostError::Identity),
    }
}

pub(super) async fn publish_job_facts(
    execution: PublisherExecution<'_>,
    request: JobFactsPublicationContext,
    parent: crate::journal::ExecutionPublicationParent,
) -> Result<JobFactsPublicationRecord, HostError> {
    match Box::pin(publish_request(
        execution,
        PublicationRequest::JobFacts(request),
        parent,
    ))
    .await?
    {
        PublicationRecord::JobFacts(value) => Ok(*value),
        PublicationRecord::Control(_) => Err(HostError::Identity),
    }
}

async fn publish_request(
    execution: PublisherExecution<'_>,
    request: PublicationRequest,
    parent: crate::journal::ExecutionPublicationParent,
) -> Result<PublicationRecord, HostError> {
    let PublisherExecution {
        docker,
        journal,
        selected,
        images,
        probe,
        metadata,
        metadata_volume,
        deadline,
    } = execution;
    let root_path = fresh_engine(docker, selected, images, deadline).await?;
    let instance_id = bounded(deadline, journal.instance_id()).await??;
    let projection = PublicationProjection::build(
        request.operation_id().to_owned(),
        &instance_id,
        selected.engine_id(),
        &root_path,
        metadata_volume.name(),
        probe.image(),
        request.clone(),
    )?;
    if probe.evidence().engine_id() != selected.engine_id()
        || probe.evidence().docker_root_digest() != selected.docker_root_digest()
        || projection.digest.is_empty()
    {
        return Err(HostError::Identity);
    }
    let recovery_projection = PublicationRecoveryProjection::from_live(
        &projection,
        metadata_volume.id(),
        metadata_volume.mountpoint(),
        &parent,
    )?
    .encode()?;
    let context = ExecutionPublicationSeed {
        operation_id: projection.operation_id.clone(),
        instance_id,
        engine_id: selected.engine_id().to_owned(),
        docker_root_digest: selected.docker_root_digest().to_owned(),
        source_revision: probe.evidence().source_revision().to_owned(),
        runtime_image_id: probe.image().runtime_id().to_owned(),
        image_binding_digest: probe.image().binding_fingerprint().to_owned(),
        projection_digest: projection.digest.clone(),
        recovery_projection_json: recovery_projection,
        parent,
    }
    .into_context(metadata)?;
    Box::pin(run::run_claimed(
        PublisherExecution {
            docker,
            journal,
            selected,
            images,
            probe,
            metadata,
            metadata_volume,
            deadline,
        },
        projection,
        context,
    ))
    .await
}

async fn fresh_engine(
    docker: &Docker,
    selected: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    deadline: Instant,
) -> Result<String, HostError> {
    bounded(deadline, images.verify_for_engine(docker, selected)).await??;
    let info = bounded(deadline, docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let root_path = info.docker_root_dir.as_deref().ok_or(HostError::Identity)?;
    let root = DockerRoot::parse(root_path)?;
    if info.id.as_deref() != Some(selected.engine_id())
        || root.digest() != selected.docker_root_digest()
    {
        return Err(HostError::Ownership);
    }
    Ok(root_path.to_owned())
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
