//! Recover one interrupted publisher operation.

use super::{Docker, HostError, Journal, ProbePhase, ProbeRow, ProbeWork};
use crate::launch::resource_probe::{lifecycle::Deadline, projection::DockerRoot};

mod container;
mod prepare;
use container::{recover_container, resolve_container_id};
use prepare::load_recovery_state;

pub(super) struct RecoveryContext<'a> {
    pub(super) docker: &'a Docker,
    pub(super) journal: &'a Journal,
    pub(super) deadline: &'a Deadline,
    pub(super) row: &'a ProbeRow,
}

pub(super) struct RecoveryState {
    pub(super) projection: super::PublicationProjection,
    pub(super) volume_name: String,
    pub(super) volume_mountpoint: String,
}

/// Remove one interrupted helper after fresh identity and projection checks.
pub(in crate::launch::resource_probe) async fn recover_active(
    docker: &Docker,
    journal: &Journal,
    info: &bollard::models::SystemInfo,
    root: &DockerRoot,
    deadline: &Deadline,
    row: &ProbeRow,
) -> Result<(), HostError> {
    let context = RecoveryContext {
        docker,
        journal,
        deadline,
        row,
    };
    validate_recovery_row(row)?;
    verify_fresh_recovery_engine(docker, row, deadline).await?;
    if row.phase == ProbePhase::Prepared {
        return deadline
            .within(journal.abort_prepared_execution_publication(&row.operation_id))
            .await?;
    }
    if info.id.as_deref() != Some(row.engine_id.as_str()) || root.digest() != row.docker_root_digest
    {
        return Err(HostError::Identity);
    }
    let state = load_recovery_state(&context, root).await?;
    let phase = deadline
        .within(journal.begin_execution_publication_recovery(&row.operation_id))
        .await??;
    if phase != row.phase {
        return Err(HostError::Ownership);
    }
    let id = resolve_container_id(&context, &state).await?;
    recover_container(&context, &state, &id).await
}

fn validate_recovery_row(row: &ProbeRow) -> Result<(), HostError> {
    if row.helper_role() == crate::journal::HostHelperRole::ExecutionPublication
        && matches!(row.work(), ProbeWork::ExecutionPublication)
        && row.operation_name == format!("velnor-execution-publication-{}", row.operation_id)
        && row.phase != ProbePhase::Quarantined
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

pub(super) async fn quarantine_context(context: &RecoveryContext<'_>) -> Result<(), HostError> {
    quarantine(context.journal, context.row, context.deadline).await
}

pub(super) async fn verify_fresh_recovery_engine(
    docker: &Docker,
    row: &ProbeRow,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let info = deadline
        .docker(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    if !fresh_identity_matches(
        info.id.as_deref(),
        info.docker_root_dir.as_deref(),
        &row.engine_id,
        &row.docker_root_digest,
    ) {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn fresh_identity_matches(
    engine_id: Option<&str>,
    root_path: Option<&str>,
    expected_engine_id: &str,
    expected_root_digest: &str,
) -> bool {
    let Some(root_path) = root_path else {
        return false;
    };
    DockerRoot::parse(root_path).is_ok_and(|root| {
        engine_id == Some(expected_engine_id) && root.digest() == expected_root_digest
    })
}

async fn quarantine(
    journal: &Journal,
    row: &ProbeRow,
    deadline: &Deadline,
) -> Result<(), HostError> {
    deadline
        .within(journal.quarantine_execution_publication(&row.operation_id, row.phase))
        .await??;
    Ok(())
}

pub(super) fn deadline_instant(deadline: &Deadline) -> std::time::Instant {
    std::time::Instant::now() + deadline.remaining()
}
