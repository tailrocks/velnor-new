// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{future::Future, time::Instant};

use bollard::query_parameters::{
    AttachContainerOptionsBuilder, RemoveContainerOptionsBuilder, StartContainerOptions,
};
use tokio::io::AsyncWriteExt;

use super::super::request::{PublicationRecord, PublicationRequest};
use super::{
    PublicationProjection, PublisherExecution, VerifiedExecutionPublicationCleanup, fresh_engine,
    inspect::{matches_created, matches_exited},
    transport::{
        bounded, drain_empty_output, inspect, inspect_optional, read_attached_line, read_stdout,
        wait_success,
    },
    valid_container_id,
};
use crate::{
    error::HostError,
    journal::{Journal, ProbePhase},
};

pub(super) async fn run_claimed<T, F, Fut>(
    execution: PublisherExecution<'_>,
    projection: PublicationProjection,
    context: crate::journal::ExecutionPublicationContext,
    persist: F,
) -> Result<T, HostError>
where
    F: FnOnce(PublicationRecord) -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
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
    let (id, attached) = start_and_send(execution, &projection).await?;
    let output = wait_and_read(execution, &projection, &id, attached, persist).await?;
    remove_and_retire(execution, &projection, &id).await?;
    Ok(output)
}

async fn start_and_send(
    execution: PublisherExecution<'_>,
    projection: &PublicationProjection,
) -> Result<(String, bollard::container::AttachContainerResults), HostError> {
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
        .stdout(true)
        .stderr(true)
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
    let request_line = encode_publication_request(projection)?;
    bounded(deadline, attached.input.write_all(&request_line))
        .await?
        .map_err(|_| HostError::Docker)?;
    bounded(deadline, attached.input.flush())
        .await?
        .map_err(|_| HostError::Docker)?;
    Ok((created.id, attached))
}

fn encode_publication_request(projection: &PublicationProjection) -> Result<Vec<u8>, HostError> {
    let request = projection.request.as_ref().ok_or(HostError::Identity)?;
    let mut request_line = request.encode()?;
    request_line.push(b'\n');
    Ok(request_line)
}

fn encode_publication_acknowledgment(operation_id: &str) -> Result<Vec<u8>, HostError> {
    let mut line = serde_json::to_vec(&velnor_resource_probe::worker_binding::AcknowledgeRequest {
        operation_id: operation_id.to_owned(),
    })
    .map_err(|_| HostError::Frame)?;
    line.push(b'\n');
    Ok(line)
}

pub(super) async fn complete_attached_publication<T, F, Fut>(
    operation_id: &str,
    request: &PublicationRequest,
    attached: &mut bollard::container::AttachContainerResults,
    deadline: Instant,
    persist: F,
) -> Result<(Vec<u8>, T), HostError>
where
    F: FnOnce(PublicationRecord) -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
    let attached_line = read_attached_line(&mut attached.output, deadline).await?;
    let validated = request.decode_record(&attached_line)?;
    let output = bounded(deadline, persist(validated)).await??;
    let ack_line = encode_publication_acknowledgment(operation_id)?;
    bounded(deadline, attached.input.write_all(&ack_line))
        .await?
        .map_err(|_| HostError::Docker)?;
    bounded(deadline, attached.input.shutdown())
        .await?
        .map_err(|_| HostError::Docker)?;
    drain_empty_output(&mut attached.output, deadline).await?;
    Ok((attached_line, output))
}

async fn wait_and_read<T, F, Fut>(
    execution: PublisherExecution<'_>,
    projection: &PublicationProjection,
    id: &str,
    mut attached: bollard::container::AttachContainerResults,
    persist: F,
) -> Result<T, HostError>
where
    F: FnOnce(PublicationRecord) -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
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
    let request = projection.request.as_ref().ok_or(HostError::Identity)?;
    let (attached_line, output) = complete_attached_publication(
        &projection.operation_id,
        request,
        &mut attached,
        deadline,
        persist,
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
    if stdout != attached_line {
        return Err(HostError::Frame);
    }
    let _record = request.decode_record(&stdout)?;
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
    Ok(output)
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
