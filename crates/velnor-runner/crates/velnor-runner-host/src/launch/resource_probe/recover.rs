//! Restart cleanup: reuse no sample and delete only one exact owned container.

use bollard::{Docker, models::SystemInfo};

use crate::error::HostError;
use crate::journal::{Journal, ProbePhase, ProbeRow};

use super::inspect;
use super::lifecycle::Deadline;
use super::ops;
use super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage};

pub(super) async fn active(
    docker: &Docker,
    journal: &Journal,
    info: &SystemInfo,
    root: &DockerRoot,
    deadline: &Deadline,
    row: ProbeRow,
) -> Result<(), HostError> {
    if row.phase == ProbePhase::Quarantined {
        return Err(HostError::Ownership);
    }
    let instance_id = deadline.within(journal.instance_id()).await??;
    let engine_id = info.id.as_deref().ok_or(HostError::Identity)?;
    if row.instance_id != instance_id
        || row.engine_id != engine_id
        || row.docker_root_digest != root.digest()
    {
        return quarantine(journal, &row).await;
    }
    let projection = persisted_projection(&row, root)?;
    if let Err(error) = cleanup(docker, journal, &row, &projection, deadline).await {
        let _quarantined = journal.quarantine_probe(&row.operation_id).await;
        return Err(error);
    }
    Ok(())
}

pub(super) async fn cleanup(
    docker: &Docker,
    journal: &Journal,
    row: &ProbeRow,
    projection: &ProbeProjection,
    deadline: &Deadline,
) -> Result<(), HostError> {
    verify_bound_engine(docker, row, projection, deadline).await?;
    if row.container_id.is_none() {
        return bind_created_by_name(docker, journal, row, projection, deadline).await;
    }
    cleanup_recorded(docker, journal, row, projection, deadline).await
}

async fn bind_created_by_name(
    docker: &Docker,
    journal: &Journal,
    row: &ProbeRow,
    projection: &ProbeProjection,
    deadline: &Deadline,
) -> Result<(), HostError> {
    if row.phase != ProbePhase::CreateRequested {
        return quarantine(journal, row).await;
    }
    let Some(response) = inspect::by_reference(docker, &projection.name, deadline).await? else {
        return journal
            .confirm_probe_absent(&row.operation_id, row.phase)
            .await;
    };
    let id = response.id.as_deref().ok_or(HostError::Ownership)?;
    if !inspect::matches(projection, &response, id) || !inspect::created(&response) {
        return quarantine(journal, row).await;
    }
    journal.bind_probe_container(&row.operation_id, id).await?;
    let mut bound = row.clone();
    bound.container_id = Some(id.to_owned());
    bound.phase = ProbePhase::ContainerCreated;
    cleanup_recorded(docker, journal, &bound, projection, deadline).await
}

async fn cleanup_recorded(
    docker: &Docker,
    journal: &Journal,
    row: &ProbeRow,
    projection: &ProbeProjection,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let id = row.container_id.as_deref().ok_or(HostError::Ownership)?;
    let Some(response) = inspect_pair(docker, projection, id, deadline).await? else {
        return journal
            .confirm_probe_absent(&row.operation_id, row.phase)
            .await;
    };
    let phase = if inspect::running(&response) {
        let Some(phase) = stop_owned(docker, journal, row, projection, id, deadline).await? else {
            return Ok(());
        };
        phase
    } else if !inspect::created(&response) && !inspect::exited(&response) {
        return quarantine(journal, row).await;
    } else {
        row.phase
    };
    remove_owned(docker, journal, row, projection, id, phase, deadline).await
}

async fn stop_owned(
    docker: &Docker,
    journal: &Journal,
    row: &ProbeRow,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<Option<ProbePhase>, HostError> {
    if row.phase == ProbePhase::RemoveRequested || !supports_stop(row.phase) {
        return quarantine(journal, row).await;
    }
    if row.phase != ProbePhase::StopRequested {
        journal
            .transition_probe(&row.operation_id, row.phase, ProbePhase::StopRequested)
            .await?;
    }
    verify_bound_engine(docker, row, projection, deadline).await?;
    let _stopped = ops::stop(docker, id, deadline).await;
    verify_bound_engine(docker, row, projection, deadline).await?;
    let Some(response) = inspect_pair(docker, projection, id, deadline).await? else {
        journal
            .confirm_probe_absent(&row.operation_id, ProbePhase::StopRequested)
            .await?;
        return Ok(None);
    };
    if inspect::running(&response) || !inspect::created_or_exited(&response) {
        return quarantine(journal, row).await;
    }
    Ok(Some(ProbePhase::StopRequested))
}

async fn remove_owned(
    docker: &Docker,
    journal: &Journal,
    row: &ProbeRow,
    projection: &ProbeProjection,
    id: &str,
    phase: ProbePhase,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let removal_phase = if phase == ProbePhase::RemoveRequested {
        ProbePhase::RemoveRequested
    } else {
        journal
            .transition_probe(&row.operation_id, phase, ProbePhase::RemoveRequested)
            .await?;
        ProbePhase::RemoveRequested
    };
    verify_bound_engine(docker, row, projection, deadline).await?;
    let _removed = ops::remove(docker, id, deadline).await;
    verify_bound_engine(docker, row, projection, deadline).await?;
    let absent = inspect_pair(docker, projection, id, deadline)
        .await?
        .is_none();
    if !absent {
        let _retry = ops::remove(docker, id, deadline).await;
    }
    if inspect_pair(docker, projection, id, deadline)
        .await?
        .is_some()
    {
        return Err(HostError::Cleanup);
    }
    verify_bound_engine(docker, row, projection, deadline).await?;
    journal
        .confirm_probe_absent(&row.operation_id, removal_phase)
        .await
}

async fn verify_bound_engine(
    docker: &Docker,
    row: &ProbeRow,
    projection: &ProbeProjection,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let info = deadline
        .docker(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let engine_id = info.id.as_deref().ok_or(HostError::Identity)?;
    let root = DockerRoot::parse(info.docker_root_dir.as_deref().ok_or(HostError::Path)?)?;
    if engine_id == row.engine_id
        && engine_id == projection.engine_id
        && root.digest() == row.docker_root_digest
        && root.digest() == projection.root.digest()
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn inspect_pair(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<Option<bollard::models::ContainerInspectResponse>, HostError> {
    let by_id = inspect::by_reference(docker, id, deadline).await?;
    let by_name = inspect::by_reference(docker, &projection.name, deadline).await?;
    match (by_id, by_name) {
        (None, None) => Ok(None),
        (Some(id_response), Some(name_response))
            if inspect::matches(projection, &id_response, id)
                && inspect::matches(projection, &name_response, id) =>
        {
            Ok(Some(id_response))
        }
        _ => Err(HostError::Ownership),
    }
}

fn persisted_projection(row: &ProbeRow, root: &DockerRoot) -> Result<ProbeProjection, HostError> {
    let image = VerifiedProbeImage::from_journal(
        row.runtime_image_id.clone(),
        row.source_revision.clone(),
        row.image_binding_digest.clone(),
    )?;
    let projection = ProbeProjection::build(
        row.operation_id.clone(),
        row.instance_id.clone(),
        row.engine_id.clone(),
        root.path(),
        image,
    )?;
    if row.role != "resource-probe"
        || row.operation_name != projection.name
        || row.projection_digest != projection.projection_digest
    {
        return Err(HostError::Ownership);
    }
    Ok(projection)
}

fn supports_stop(phase: ProbePhase) -> bool {
    matches!(
        phase,
        ProbePhase::StartRequested
            | ProbePhase::Started
            | ProbePhase::WaitRequested
            | ProbePhase::StopRequested
    )
}

async fn quarantine<T>(journal: &Journal, row: &ProbeRow) -> Result<T, HostError> {
    if row.phase != ProbePhase::Quarantined {
        journal.quarantine_probe(&row.operation_id).await?;
    }
    Err(HostError::Ownership)
}
