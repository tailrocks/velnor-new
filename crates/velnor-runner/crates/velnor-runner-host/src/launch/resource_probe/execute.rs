//! One current-process probe attempt; samples are returned only after cleanup.

use std::time::{Duration, Instant};

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
    pub(super) container_host_pid: u32,
    pub(super) sample_generation_id: String,
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
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
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
    let start_result = ops::start(docker, id, deadline).await;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let container_host_pid = if start_result.is_err() {
        verify_started_after_lost_reply(docker, projection, id, deadline).await?
    } else {
        verify_started(docker, projection, id, deadline).await?
    };
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
    if record.probe_host_pid != container_host_pid
        || record.probe_namespace_pid != container_host_pid
    {
        return Err(HostError::Identity);
    }
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
        container_host_pid,
        sample_generation_id: projection.operation_id.clone(),
        observed_at,
    })
}

async fn verify_started_after_lost_reply(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<u32, HostError> {
    verify_started_pid(docker, projection, id, deadline)
        .await
        .map_err(|_| HostError::ContainerStartUncertain)
}

async fn verify_started(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<u32, HostError> {
    verify_started_pid(docker, projection, id, deadline).await
}

async fn verify_started_pid(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<u32, HostError> {
    let inspection_started = Instant::now();
    loop {
        let response = inspect::by_reference(docker, id, deadline)
            .await?
            .ok_or(HostError::ContainerStartUncertain)?;
        if !inspect::matches(projection, &response, id) {
            return Err(HostError::Ownership);
        }
        if let Some(pid) = inspect::running_host_pid(&response) {
            return Ok(pid);
        }
        if inspect::exited(&response) || inspection_started.elapsed() >= Duration::from_secs(2) {
            return Err(HostError::ContainerStartUncertain);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
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
