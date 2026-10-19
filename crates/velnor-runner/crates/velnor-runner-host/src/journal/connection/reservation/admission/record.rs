//! Reservation persistence helpers.

use super::{
    AssignmentIdentity, AssignmentKey, ConnectionJournal, HostError, ReservationAttempt,
    ReservationRequest,
};

pub(super) fn runner_assignment_fields(key: &AssignmentKey) -> (Option<i64>, Option<String>) {
    match key {
        AssignmentKey::AssignedRequest { request_id } => {
            (Some(*request_id), Some(format!("v{request_id}")))
        }
        AssignmentKey::PopulationMessage { message_id } => (None, Some(format!("m{message_id}"))),
        AssignmentKey::Session { name } => (None, Some(name.clone())),
        AssignmentKey::Remote { .. } => (None, None),
    }
}

pub(super) async fn insert_launch_intent(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    attempt: &ReservationAttempt<'_>,
    request_id: Option<i64>,
    runner_name: Option<String>,
) -> Result<i64, HostError> {
    connection.execute(
        "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id, runner_request_id, runner_name, assignment_key) VALUES (?1, 'launch', ?2, 'pending', ?3, ?4, ?5, ?2)",
        (
            journal.connection_id.as_str(),
            attempt.assignment_key.as_str(),
            attempt.request.assignment.scale_set_id(),
            request_id,
            runner_name,
        ),
    ).await.map_err(|_| HostError::Journal)?;
    Ok(connection.last_insert_rowid())
}

pub(super) async fn insert_capacity_reservation(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    attempt: &ReservationAttempt<'_>,
    intent_id: i64,
) -> Result<i64, HostError> {
    connection.execute(
        "INSERT INTO capacity_reservations (intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'reserved')",
        (
            intent_id,
            journal.connection_id.as_str(),
            attempt.assignment_key.as_str(),
            attempt.request.engine_id,
            attempt.request.docker_root_digest,
            journal.binding_digest.as_str(),
            attempt.request.sample_generation_id,
            attempt.cpu,
            attempt.memory,
        ),
    ).await.map_err(|_| HostError::Journal)?;
    Ok(connection.last_insert_rowid())
}

pub(super) async fn validate_bound_identity(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    identity: &AssignmentIdentity,
) -> Result<(), HostError> {
    if identity.connection_id() != scope.connection_id() {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [scope.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let id: Option<i64> = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if id == Some(identity.scale_set_id()) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn capacity_available(
    connection: &turso::Connection,
    request: ReservationRequest<'_>,
    storage_memory_bytes: u64,
) -> Result<bool, HostError> {
    let limits = super::super::storage::capacity_limits(connection).await?;
    let usage = super::super::storage::capacity_usage(connection).await?;
    let requested_memory = request
        .memory_bytes
        .checked_add(storage_memory_bytes)
        .ok_or(HostError::Journal)?;
    Ok(usage.jobs < limits.jobs
        && usage
            .cpu_millicores
            .checked_add(request.cpu_millicores)
            .is_some_and(|sum| sum <= limits.cpu_millicores)
        && usage
            .memory_bytes
            .checked_add(requested_memory)
            .is_some_and(|sum| sum <= limits.memory_bytes))
}
