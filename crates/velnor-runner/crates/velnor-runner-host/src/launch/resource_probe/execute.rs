//! One current-process probe attempt; samples are returned only after cleanup.

use std::time::Instant;

use bollard::Docker;

use crate::error::HostError;
use crate::journal::{Journal, ProbePhase};

use super::inspect;
use super::lifecycle::Deadline;
use super::ops;
use super::projection::ProbeProjection;
use super::record::ProbeRecord;
use super::recover;

pub(super) struct ObservedProbeRecord {
    pub(super) record: ProbeRecord,
    pub(super) observed_at: Instant,
}

pub(super) async fn run(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    memory_total: u64,
    deadline: &Deadline,
) -> Result<ObservedProbeRecord, HostError> {
    let result = execute(docker, journal, projection, memory_total, deadline).await;
    if result.is_err() {
        recover_current(docker, journal, projection, deadline).await;
    }
    result
}

async fn execute(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    memory_total: u64,
    deadline: &Deadline,
) -> Result<ObservedProbeRecord, HostError> {
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        ))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let id = ops::create(docker, projection, deadline).await?;
    deadline
        .within(journal.bind_probe_container(&projection.operation_id, &id))
        .await??;
    verify_created(docker, projection, &id, deadline).await?;
    let record = execute_started(docker, journal, projection, &id, memory_total, deadline).await?;
    let row = deadline
        .within(journal.active_probe())
        .await??
        .ok_or(HostError::Journal)?;
    recover::cleanup(docker, journal, &row, projection, deadline).await?;
    Ok(record)
}

async fn verify_created(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let by_id = inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    let by_name = inspect::by_reference(docker, &projection.name, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    if inspect::matches(projection, &by_id, id)
        && inspect::matches(projection, &by_name, id)
        && inspect::created(&by_id)
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn execute_started(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    id: &str,
    memory_total: u64,
    deadline: &Deadline,
) -> Result<ObservedProbeRecord, HostError> {
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::StartRequested,
        ))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    if ops::start(docker, id, deadline).await.is_err() {
        verify_started_after_lost_reply(docker, projection, id, deadline).await?;
    }
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::StartRequested,
            ProbePhase::Started,
        ))
        .await??;
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::Started,
            ProbePhase::WaitRequested,
        ))
        .await??;
    ops::wait_success(docker, id, deadline).await?;
    verify_exit(docker, projection, id, deadline).await?;
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::WaitRequested,
            ProbePhase::Waited,
        ))
        .await??;
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::Waited,
            ProbePhase::LogsRequested,
        ))
        .await??;
    let output = ops::stdout(docker, id, deadline).await?;
    let record = ProbeRecord::parse(&output, memory_total).ok_or(HostError::Frame)?;
    let observed_at = Instant::now();
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::LogsRequested,
            ProbePhase::OutputObserved,
        ))
        .await??;
    Ok(ObservedProbeRecord {
        record,
        observed_at,
    })
}

async fn verify_started_after_lost_reply(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let response = inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::ContainerStartUncertain)?;
    if inspect::matches(projection, &response, id)
        && (inspect::running(&response) || inspect::exited(&response))
    {
        Ok(())
    } else {
        Err(HostError::ContainerStartUncertain)
    }
}

async fn verify_exit(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let response = inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    if inspect::matches(projection, &response, id) && inspect::exited_successfully(&response) {
        Ok(())
    } else {
        Err(HostError::Docker)
    }
}

async fn recover_current(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    deadline: &Deadline,
) {
    let Ok(Ok(Some(row))) = deadline.within(journal.active_probe()).await else {
        return;
    };
    if row.phase == ProbePhase::Prepared {
        let _aborted = deadline
            .within(journal.abort_prepared_probe(&row.operation_id))
            .await;
        return;
    }
    if let Err(error) = recover::cleanup(docker, journal, &row, projection, deadline).await {
        if matches!(
            error,
            HostError::DockerTimeout | HostError::Identity | HostError::Path
        ) {
            return;
        }
        let _quarantined = deadline
            .within(journal.quarantine_probe(&row.operation_id))
            .await;
    }
}
