//! Shared identity checks and SQL reads for physical runner effects.

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::request_lifecycle::RunnerJitClaim;
use crate::journal::sql::token_rejected;

pub(in crate::journal) async fn mark_runner_cleaned_after_cleanup(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<(), HostError> {
    if intent_id <= 0 {
        return Err(HostError::Journal);
    }
    connection
        .execute(
            "UPDATE connection_runner_jits SET state = 'cleaned' WHERE intent_id = ?1 AND connection_id = ?2 AND state IN ('bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed') AND runner_id IS NOT NULL AND runner_name IS NOT NULL AND EXISTS (SELECT 1 FROM capacity_reservations AS r WHERE r.intent_id = ?1 AND r.connection_id = ?2 AND r.remote_cleanup_proven = 1 AND r.local_cleanup_proven = 1 AND (NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p WHERE p.reservation_id = r.id) OR EXISTS (SELECT 1 FROM capacity_reservation_provisioning_state AS s WHERE s.reservation_id = r.id AND s.storage_cleanup_proven = 1)))",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_provisioning_state SET jit_state = 'cleaned' WHERE jit_state = 'bound' AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning AS p JOIN capacity_reservations AS r ON r.id = p.reservation_id WHERE p.operation_id = ?1 AND r.connection_id = ?2 AND r.remote_cleanup_proven = 1 AND r.local_cleanup_proven = 1) AND storage_cleanup_proven = 1 AND EXISTS (SELECT 1 FROM connection_runner_jits AS j WHERE j.intent_id = ?1 AND j.connection_id = ?2 AND j.state = 'cleaned' AND j.runner_id = capacity_reservation_provisioning_state.runner_id AND j.runner_name = capacity_reservation_provisioning_state.runner_name AND j.scale_set_id = capacity_reservation_provisioning_state.runner_scale_set_id)",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) fn validate_jit_claim(
    scope: &ConnectionJournal,
    claim: &RunnerJitClaim,
) -> Result<(), HostError> {
    if &claim.connection_id != scope.connection_id()
        || claim.scale_set_id <= 0
        || claim.intent_id <= 0
        || claim.witness_request_id <= 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) async fn physical_intent_is_fresh(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    intent_id: i64,
    requested_name: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.assignment_key FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id AND r.connection_id = i.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id AND p.operation_id = i.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id WHERE i.id = ?1 AND i.connection_id = ?2 AND i.scale_set_id = ?3 AND i.kind = 'launch' AND i.state = 'pending' AND i.cleanup_proven = 0 AND i.jit_requested = 0 AND i.acquire_attempted = 0 AND i.github_runner_id IS NULL AND i.runner_name = ?4 AND i.worker_volume IS NULL AND i.docker_id = s.runner_container_id AND i.dind_id = s.dind_container_id AND i.launch_phase = 'resources_bound' AND r.state = 'reserved' AND s.bootstrap_start_state = 'started' AND s.resources_bound = 1 AND s.instance_binding_digest IS NOT NULL AND s.jit_state = 'not_requested' AND s.publication_state = 'not_requested' AND s.delivery_state = 'not_requested' AND s.storage_cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM connection_runner_jits WHERE intent_id = ?1) AND NOT EXISTS (SELECT 1 FROM connection_intents WHERE runner_name = ?4 AND cleanup_proven = 0 AND id != ?1) AND NOT EXISTS (SELECT 1 FROM connection_runner_jits WHERE requested_name = ?4 AND state != 'cleaned')",
            (intent_id, connection_id, set_id, requested_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let key: String = row.get(0).map_err(|_| HostError::Journal)?;
    let prefix = format!("v1|{connection_id}|{set_id}|remote|6:runner|32:");
    Ok(key
        .strip_prefix(&prefix)
        .is_some_and(|nonce| valid_nonce(nonce) && requested_name == format!("v{nonce}")))
}

pub(super) async fn has_legacy_receipts(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM connection_source_authorizations AS a JOIN connection_intents AS i ON i.id = a.intent_id AND i.connection_id = a.connection_id WHERE a.connection_id = ?1 AND a.scale_set_id = ?2 AND i.cleanup_proven = 0 LIMIT 1",
            (connection_id, set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(super) async fn has_uncertain_acquire(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND acquire_state IN ('requested', 'already_acquired', 'uncertain') LIMIT 1",
            (connection_id, set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

pub(super) fn valid_runner_name(value: &str) -> bool {
    !token_rejected(value) && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
