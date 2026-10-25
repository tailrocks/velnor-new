use bollard::Docker;
use velnor_resource_probe::cleanup_observer_wire::{CleanupObserverRecord, CleanupObserverRequest};

use crate::error::HostError;
use crate::journal::{Journal, ProbePhase, ProbeRow};

use super::super::execute::ProbeCleanupEvidence;
use super::super::lifecycle::Deadline;
use super::super::projection::ProbeProjection;
use super::super::{inspect, ops, recover};
use super::record::parse_record;

pub(super) async fn run(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    request_json: &str,
    deadline: &Deadline,
) -> Result<(CleanupObserverRecord, ProbeCleanupEvidence), HostError> {
    if projection.purpose != crate::journal::ResourceProbePurpose::CleanupObserver {
        return Err(HostError::Identity);
    }
    let request: CleanupObserverRequest =
        serde_json::from_str(request_json).map_err(|_| HostError::Frame)?;
    let request_line = request_line(request_json)?;
    let result = execute(
        docker,
        journal,
        projection,
        &request,
        &request_line,
        deadline,
    )
    .await;
    if result.is_err() {
        Box::pin(recover_current(docker, journal, deadline, projection)).await;
    }
    result
}

async fn execute(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    request: &CleanupObserverRequest,
    request_line: &[u8],
    deadline: &Deadline,
) -> Result<(CleanupObserverRecord, ProbeCleanupEvidence), HostError> {
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

    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::StartRequested,
        ))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let attachment = ops::owned_cgroups(docker, projection, &id, deadline).await?;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
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
    let output =
        ops::owned_cgroups_output(docker, projection, &id, attachment, request_line, deadline)
            .await?;
    let record = parse_record(&output, request)?;
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
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::LogsRequested,
            ProbePhase::OutputObserved,
        ))
        .await??;
    let row = deadline
        .within(journal.active_probe())
        .await??
        .ok_or(HostError::Journal)?;
    let cleanup = recover::cleanup(docker, journal, &row, projection, deadline).await?;
    Ok((record, cleanup))
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

async fn recover_current(
    docker: &Docker,
    journal: &Journal,
    deadline: &Deadline,
    projection: &ProbeProjection,
) {
    let Ok(Ok(Some(row))) = deadline.within(journal.active_probe()).await else {
        return;
    };
    if !same_projection(&row, projection) {
        return;
    }
    if row.phase == ProbePhase::Prepared {
        let _aborted = deadline
            .within(journal.abort_prepared_probe(&row.operation_id))
            .await;
        return;
    }
    let Ok(Ok(current)) = deadline.within(docker.info()).await else {
        return;
    };
    let _recovered = Box::pin(recover::active(
        docker,
        journal,
        &current,
        &projection.root,
        deadline,
        row,
    ))
    .await;
}

fn same_projection(row: &ProbeRow, projection: &ProbeProjection) -> bool {
    row.operation_id == projection.operation_id
        && row.projection_digest == projection.projection_digest
        && row.engine_id == projection.engine_id
        && row.docker_root_digest == projection.root.digest()
}

fn request_line(request_json: &str) -> Result<Vec<u8>, HostError> {
    let mut line = request_json.as_bytes().to_vec();
    line.push(b'\n');
    if line.len() > 32 * 1024 {
        return Err(HostError::Frame);
    }
    Ok(line)
}
