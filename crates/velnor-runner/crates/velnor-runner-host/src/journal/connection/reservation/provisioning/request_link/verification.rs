use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

pub(super) async fn actual_request_matches_link(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    intent_id: i64,
    scale_set_id: i64,
    witness_request_id: i64,
    actual_request_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id JOIN connection_request_authorizations AS actual ON actual.connection_id = jit.connection_id AND actual.scale_set_id = jit.scale_set_id AND actual.request_id = intent.runner_request_id WHERE attempt.connection_id = ?1 AND attempt.scale_set_id = ?2 AND attempt.witness_request_id = ?3 AND attempt.reservation_id = ?4 AND reservation.intent_id = ?5 AND intent.runner_request_id = ?6 AND jit.connection_id = ?1 AND jit.scale_set_id = ?2 AND jit.witness_request_id = ?3 AND jit.requested_name = intent.runner_name AND jit.runner_name = intent.runner_name AND intent.github_runner_id = CAST(jit.runner_id AS TEXT) AND actual.binding_digest = attempt.binding_digest AND actual.authority_model = 'created_empty_exclusive_admin_session_v1' AND actual.acquire_state = 'acquired' AND ((jit.started_request_id = intent.runner_request_id AND jit.state IN ('started', 'completed', 'cleaned') AND actual.started_event_digest IS NOT NULL AND actual.started_runner_id = jit.runner_id AND actual.started_runner_name = jit.runner_name) OR (jit.completed_request_id = intent.runner_request_id AND jit.state IN ('completed', 'cleaned') AND actual.completed_event_digest IS NOT NULL AND actual.completed_runner_id = jit.runner_id AND actual.completed_runner_name = jit.runner_name AND actual.completed_intent_id = intent.id))",
            (
                scope.connection_id().as_str(),
                scale_set_id,
                witness_request_id,
                reservation_id,
                intent_id,
                actual_request_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}

pub(super) async fn reservation_intent_id(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id FROM capacity_reservations WHERE id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let intent_id = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if intent_id <= 0 {
        return Err(HostError::Journal);
    }
    Ok(intent_id)
}

pub(super) fn physical_nonce(
    key: &crate::journal::connection::AssignmentKey,
) -> Result<&str, HostError> {
    let crate::journal::connection::AssignmentKey::Remote { kind, subject } = key else {
        return Err(HostError::Identity);
    };
    if kind != "runner" || !lower_hex(subject, 32) {
        return Err(HostError::Identity);
    }
    Ok(subject)
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
