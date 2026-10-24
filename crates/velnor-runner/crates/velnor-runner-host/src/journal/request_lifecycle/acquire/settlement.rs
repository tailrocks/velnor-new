use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::journal::source_authorization::AssignedAcquireResult;

use super::{acquire_result_matches, digest, request_id, response_record, validate_scope_identity};

struct AcquireResultUpdate<'a> {
    connection_id: &'a str,
    binding_digest: &'a str,
    identity: &'a AssignmentIdentity,
    request_id: i64,
    state: &'static str,
    response_json: &'a str,
    response_digest: &'a str,
}

impl ConnectionJournal {
    /// Record a definite no-acquire result and release only its unused reservation.
    pub(crate) async fn record_request_acquire_and_settle_unused(
        &self,
        identity: &AssignmentIdentity,
        result: AssignedAcquireResult,
    ) -> Result<bool, HostError> {
        if !matches!(
            result,
            AssignedAcquireResult::NoEffect | AssignedAcquireResult::Rejected
        ) {
            return Err(HostError::Identity);
        }
        let request_id = request_id(identity, self)?;
        let (state, response_json) = response_record(&result, request_id)?;
        let response_digest = digest(response_json.as_bytes());
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        let update = AcquireResultUpdate {
            connection_id: &connection_id,
            binding_digest: &binding_digest,
            identity,
            request_id,
            state,
            response_json: &response_json,
            response_digest: &response_digest,
        };
        in_transaction(self.journal_ref(), async |connection| {
            validate_scope_identity(self, connection, identity).await?;
            if !record_result(connection, &update).await? {
                return Ok(false);
            }
            let Some((reservation_id, intent_id)) = unused_reservation(
                connection,
                &connection_id,
                &binding_digest,
                identity,
                request_id,
            )
            .await?
            else {
                return Ok(false);
            };
            release_unused_reservation(
                connection,
                &connection_id,
                reservation_id,
                intent_id,
                &response_digest,
            )
            .await?;
            Ok(true)
        })
        .await
    }
}

async fn record_result(
    connection: &turso::Connection,
    update: &AcquireResultUpdate<'_>,
) -> Result<bool, HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_request_authorizations SET acquire_state = ?1, acquire_response_json = ?2, acquire_response_digest = ?3 WHERE connection_id = ?4 AND scale_set_id = ?5 AND request_id = ?6 AND binding_digest = ?7 AND acquire_state = 'requested' AND acquire_response_json IS NULL AND acquire_response_digest IS NULL",
            (
                update.state,
                update.response_json,
                update.response_digest,
                update.connection_id,
                update.identity.scale_set_id(),
                update.request_id,
                update.binding_digest,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(true);
    }
    if acquire_result_matches(
        connection,
        update.connection_id,
        update.identity.scale_set_id(),
        update.request_id,
        update.state,
        update.response_digest,
    )
    .await?
    {
        return Ok(false);
    }
    Err(HostError::Journal)
}

async fn unused_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    binding_digest: &str,
    identity: &AssignmentIdentity,
    request_id: i64,
) -> Result<Option<(i64, i64)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT reservation.id, reservation.intent_id FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id AND reservation.connection_id = attempt.connection_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id WHERE attempt.connection_id = ?1 AND attempt.scale_set_id = ?2 AND attempt.witness_request_id = ?3 AND attempt.binding_digest = ?4 AND attempt.physical_nonce = substr(intent.runner_name, 2) AND reservation.assignment_key = intent.assignment_key AND reservation.state = 'reserved' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.kind = 'launch' AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND intent.jit_requested = 0 AND intent.acquire_attempted = 0 AND intent.runner_completed = 0 AND intent.github_runner_id IS NULL AND intent.docker_id IS NULL AND intent.dind_id IS NULL AND intent.worker_volume IS NULL AND intent.launch_phase = 'provisioning_held' AND state.bootstrap_start_state = 'not_requested' AND state.runner_container_id IS NULL AND state.dind_container_id IS NULL AND state.resources_bound = 0 AND state.jit_state = 'not_requested' AND state.runner_id IS NULL AND state.runner_name IS NULL AND state.runner_scale_set_id IS NULL AND state.publication_state = 'not_requested' AND state.delivery_state = 'not_requested' AND state.storage_cleanup_proven = 0 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = reservation.id AND object.create_state != 'unclaimed') AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bootstrap_containers AS container WHERE container.reservation_id = reservation.id AND (container.create_state != 'unclaimed' OR container.start_state != 'not_requested')) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_storage_initialization AS initialization WHERE initialization.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_networks AS network WHERE network.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_attachments AS attachment WHERE attachment.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS metadata WHERE metadata.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM connection_runner_jits AS jit WHERE jit.intent_id = reservation.intent_id) AND NOT EXISTS (SELECT 1 FROM execution_publication_operations AS publication WHERE publication.reservation_id = reservation.id) AND NOT EXISTS (SELECT 1 FROM connection_execution_proofs AS proof WHERE proof.reservation_id = reservation.id)",
            (
                connection_id,
                identity.scale_set_id(),
                request_id,
                binding_digest,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let result = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(result))
}

async fn release_unused_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    intent_id: i64,
    response_digest: &str,
) -> Result<(), HostError> {
    mark_cleanup_proven(connection, connection_id, reservation_id).await?;
    if !crate::journal::connection::reservation_release_ready(connection, reservation_id).await? {
        return Err(HostError::Journal);
    }
    crate::journal::connection::record_no_acquire_outcome(
        connection,
        reservation_id,
        response_digest,
    )
    .await?;
    release_reservation(connection, connection_id, reservation_id, intent_id).await
}

async fn mark_cleanup_proven(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<(), HostError> {
    for column in ["remote_cleanup_proven", "local_cleanup_proven"] {
        let changed = connection
            .execute(
                &format!("UPDATE capacity_reservations SET {column} = 1 WHERE id = ?1 AND connection_id = ?2 AND state = 'reserved' AND {column} = 0"),
                (reservation_id, connection_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn release_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    intent_id: i64,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE capacity_reservations SET state = 'released' WHERE id = ?1 AND connection_id = ?2 AND intent_id = ?3 AND state = 'reserved' AND remote_cleanup_proven = 1 AND local_cleanup_proven = 1",
            (reservation_id, connection_id, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "UPDATE connection_intents SET cleanup_proven = 1 WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        let retired = connection
            .execute(
                "DELETE FROM physical_attempt_current WHERE reservation_id = ?1 AND intent_id = ?2",
                (reservation_id, intent_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if retired == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    } else {
        Err(HostError::Journal)
    }
}
