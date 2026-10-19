//! Execute one bounded publication helper lifecycle.

use std::time::Instant;

use bollard::query_parameters::{
    AttachContainerOptionsBuilder, RemoveContainerOptionsBuilder, StartContainerOptions,
};
use tokio::io::AsyncWriteExt;

use super::super::request::PublicationRecord;
use super::{
    PublicationProjection, PublisherExecution, VerifiedExecutionPublicationCleanup, fresh_engine,
    inspect::{matches_created, matches_exited},
    transport::{bounded, inspect, inspect_optional, read_stdout, wait_success},
    valid_container_id,
};
use crate::{
    error::HostError,
    journal::{Journal, ProbePhase},
};

pub(super) async fn run_claimed(
    execution: PublisherExecution<'_>,
    projection: PublicationProjection,
    context: crate::journal::ExecutionPublicationContext,
) -> Result<PublicationRecord, HostError> {
    let PublisherExecution {
        journal, deadline, ..
    } = execution;
    bounded(deadline, journal.prepare_execution_publication(context)).await??;
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::Prepared,
        ProbePhase::CreateRequested,
    )
    .await?;
    let id = start_and_send(execution, &projection).await?;
    let record = wait_and_read(execution, &projection, &id).await?;
    remove_and_retire(execution, &projection, &id).await?;
    Ok(record)
}

async fn start_and_send(
    execution: PublisherExecution<'_>,
    projection: &PublicationProjection,
) -> Result<String, HostError> {
    let PublisherExecution {
        docker,
        journal,
        selected,
        images,
        metadata_volume,
        deadline,
        ..
    } = execution;
    fresh_engine(docker, selected, images, deadline).await?;
    let created = bounded(
        deadline,
        docker.create_container(Some(projection.options.clone()), projection.config.clone()),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    if !valid_container_id(&created.id) {
        return Err(HostError::Identity);
    }
    bounded(
        deadline,
        journal.bind_execution_publication_container(&projection.operation_id, &created.id),
    )
    .await??;
    let attach_options = AttachContainerOptionsBuilder::new()
        .stdin(true)
        .stdout(false)
        .stderr(false)
        .stream(true)
        .logs(false)
        .build();
    let mut attached = bounded(
        deadline,
        docker.attach_container(&created.id, Some(attach_options)),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    let before_start = inspect(docker, &created.id, deadline).await?;
    if !matches_created(projection, metadata_volume, &before_start, &created.id) {
        return Err(HostError::Ownership);
    }
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::ContainerCreated,
        ProbePhase::StartRequested,
    )
    .await?;
    fresh_engine(docker, selected, images, deadline).await?;
    bounded(
        deadline,
        docker.start_container(&created.id, None::<StartContainerOptions>),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::StartRequested,
        ProbePhase::Started,
    )
    .await?;
    let request = projection.request.as_ref().ok_or(HostError::Identity)?;
    let mut request_line = request.encode()?;
    request_line.push(b'\n');
    bounded(deadline, attached.input.write_all(&request_line))
        .await?
        .map_err(|_| HostError::Docker)?;
    bounded(deadline, attached.input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;
    drop(attached.output);
    Ok(created.id)
}

async fn wait_and_read(
    execution: PublisherExecution<'_>,
    projection: &PublicationProjection,
    id: &str,
) -> Result<PublicationRecord, HostError> {
    let PublisherExecution {
        docker,
        journal,
        selected,
        images,
        metadata_volume,
        deadline,
        ..
    } = execution;
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::Started,
        ProbePhase::WaitRequested,
    )
    .await?;
    wait_success(docker, id, deadline).await?;
    let after_wait = inspect(docker, id, deadline).await?;
    if !matches_exited(projection, metadata_volume, &after_wait, id) {
        return Err(HostError::Ownership);
    }
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::WaitRequested,
        ProbePhase::Waited,
    )
    .await?;
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::Waited,
        ProbePhase::LogsRequested,
    )
    .await?;
    fresh_engine(docker, selected, images, deadline).await?;
    let stdout = read_stdout(docker, id, deadline).await?;
    let request = projection.request.as_ref().ok_or(HostError::Identity)?;
    let record = request.decode_record(&stdout)?;
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::LogsRequested,
        ProbePhase::OutputObserved,
    )
    .await?;
    transition(
        journal,
        deadline,
        &projection.operation_id,
        ProbePhase::OutputObserved,
        ProbePhase::RemoveRequested,
    )
    .await?;
    Ok(record)
}

async fn remove_and_retire(
    execution: PublisherExecution<'_>,
    projection: &PublicationProjection,
    id: &str,
) -> Result<(), HostError> {
    let PublisherExecution {
        docker,
        journal,
        selected,
        images,
        deadline,
        ..
    } = execution;
    fresh_engine(docker, selected, images, deadline).await?;
    let before_remove = inspect(docker, id, deadline).await?;
    if !matches_exited(projection, execution.metadata_volume, &before_remove, id) {
        return Err(HostError::Ownership);
    }
    bounded(
        deadline,
        docker.remove_container(
            id,
            Some(
                RemoveContainerOptionsBuilder::new()
                    .force(false)
                    .v(false)
                    .build(),
            ),
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    if inspect_optional(docker, id, deadline).await?.is_some() {
        return Err(HostError::Ownership);
    }
    let cleanup = VerifiedExecutionPublicationCleanup::after_exact_removal(
        projection,
        id,
        ProbePhase::RemoveRequested,
    )?;
    bounded(deadline, journal.retire_execution_publication(cleanup)).await??;
    Ok(())
}

async fn transition(
    journal: &Journal,
    deadline: Instant,
    operation_id: &str,
    from: ProbePhase,
    to: ProbePhase,
) -> Result<(), HostError> {
    bounded(
        deadline,
        journal.transition_execution_publication(operation_id, from, to),
    )
    .await??;
    Ok(())
}
