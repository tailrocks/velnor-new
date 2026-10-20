use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;

use crate::journal::connection::ConnectionJournal;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalAttemptIdentity {
    pub(crate) lineage_id: i64,
    pub(crate) generation: i64,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) witness_request_id: i64,
    pub(crate) physical_nonce: String,
    pub(crate) binding_digest: String,
    pub(crate) source_digest: String,
    pub(crate) acquire_response_digest: Option<String>,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
}

impl ConnectionJournal {
    pub(crate) async fn current_physical_attempt(
        &self,
        reservation_id: i64,
    ) -> Result<Option<PhysicalAttemptIdentity>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT attempt.lineage_id, attempt.generation, attempt.reservation_id, attempt.intent_id, attempt.witness_request_id, attempt.physical_nonce, attempt.binding_digest, attempt.source_digest, attempt.acquire_response_digest, attempt.engine_id, attempt.docker_root_digest FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id WHERE current.reservation_id = ?1 AND attempt.connection_id = ?2 AND attempt.binding_digest = ?3",
                    (reservation_id, self.connection_id().as_str(), self.binding_digest()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let attempt = PhysicalAttemptIdentity {
                lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
                generation: row.get(1).map_err(|_| HostError::Journal)?,
                reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
                intent_id: row.get(3).map_err(|_| HostError::Journal)?,
                witness_request_id: row.get(4).map_err(|_| HostError::Journal)?,
                physical_nonce: row.get(5).map_err(|_| HostError::Journal)?,
                binding_digest: row.get(6).map_err(|_| HostError::Journal)?,
                source_digest: row.get(7).map_err(|_| HostError::Journal)?,
                acquire_response_digest: row.get(8).map_err(|_| HostError::Journal)?,
                engine_id: row.get(9).map_err(|_| HostError::Journal)?,
                docker_root_digest: row.get(10).map_err(|_| HostError::Journal)?,
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(attempt))
        })
        .await
    }
}

pub(super) async fn record_no_acquire_outcome(
    connection: &turso::Connection,
    reservation_id: i64,
    response_digest: &str,
) -> Result<(), HostError> {
    if reservation_id <= 0
        || response_digest.len() != 64
        || !response_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT lineage_id, generation FROM physical_attempt_current WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let lineage_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_outcomes (lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at) VALUES (?1, ?2, ?3, 'acquire_no_effect', ?4, ?4, unixepoch())",
            (lineage_id, generation, reservation_id, response_digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn record_terminal_cleanup_outcome(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    intent_id: i64,
) -> Result<bool, HostError> {
    use sha2::{Digest, Sha256};

    let mut rows = connection
        .query(
            "SELECT attempt.lineage_id, attempt.generation, attempt.binding_digest, state.instance_binding_digest, cleanup.instance_binding_digest, completion.runner_request_id, completion.runner_id, completion.runner_name, auth.completed_event_digest FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id JOIN capacity_reservations AS reservation ON reservation.id = current.reservation_id JOIN connection_intents AS intent ON intent.id = current.intent_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = reservation.id JOIN connection_completion_cleanup AS completion ON completion.intent_id = intent.id AND completion.connection_id = intent.connection_id AND completion.scale_set_id = intent.scale_set_id AND completion.runner_absent = 1 AND completion.lease_until > CAST(strftime('%s', 'now') AS INTEGER) JOIN connection_request_authorizations AS auth ON auth.connection_id = completion.connection_id AND auth.scale_set_id = completion.scale_set_id AND auth.request_id = completion.runner_request_id WHERE current.reservation_id = ?1 AND current.intent_id = ?2 AND attempt.connection_id = ?3 AND reservation.state IN ('reserved', 'uncertain') AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1 AND intent.cleanup_proven = 1 AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1 AND state.instance_binding_digest = cleanup.instance_binding_digest AND completion.runner_id > 0 AND completion.runner_name != '' AND auth.acquire_state = 'acquired' AND auth.completed_event_digest IS NOT NULL",
            (reservation_id, intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let lineage_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let binding: String = row.get(2).map_err(|_| HostError::Journal)?;
    let state_binding: String = row.get(3).map_err(|_| HostError::Journal)?;
    let cleanup_binding: String = row.get(4).map_err(|_| HostError::Journal)?;
    let request_id: i64 = row.get(5).map_err(|_| HostError::Journal)?;
    let runner_id: i64 = row.get(6).map_err(|_| HostError::Journal)?;
    let runner_name: String = row.get(7).map_err(|_| HostError::Journal)?;
    let event_digest: String = row.get(8).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || lineage_id <= 0
        || generation <= 0
        || binding.len() != 64
        || state_binding != cleanup_binding
        || request_id <= 0
        || runner_id <= 0
        || runner_name.is_empty()
        || event_digest.len() != 64
    {
        return Err(HostError::Journal);
    }
    let canonical = format!(
        "{connection_id}:{reservation_id}:{intent_id}:{lineage_id}:{generation}:{binding}:{state_binding}:{request_id}:{runner_id}:{runner_name}:{event_digest}"
    );
    let digest = Sha256::digest(canonical.as_bytes());
    let digest = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let changed = connection
        .execute(
            "INSERT INTO physical_attempt_outcomes (lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at) VALUES (?1, ?2, ?3, 'normal_completion', ?4, ?4, unixepoch())",
            (lineage_id, generation, reservation_id, digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(true)
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn record_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    intent_id: i64,
    witness_request_id: i64,
    physical_nonce: &str,
) -> Result<PhysicalAttemptIdentity, HostError> {
    if reservation_id <= 0 || intent_id <= 0 || witness_request_id <= 0 {
        return Err(HostError::Identity);
    }
    let context = attempt_context(
        connection,
        scope,
        reservation_id,
        intent_id,
        witness_request_id,
        physical_nonce,
    )
    .await?;
    let (lineage_id, generation) = match reusable_lineage(connection, scope).await? {
        Some(reusable) => reusable,
        None => (create_lineage(connection, scope).await?, 1),
    };
    connection
        .execute(
            "INSERT INTO physical_attempts (lineage_id, generation, reservation_id, intent_id, physical_nonce, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, acquire_response_digest, engine_id, docker_root_digest, resource_spec_digest, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL, unixepoch())",
            (
                lineage_id,
                generation,
                reservation_id,
                intent_id,
                physical_nonce,
                scope.connection_id().as_str(),
                context.scale_set_id,
                witness_request_id,
                scope.binding_digest(),
                context.source_digest.as_str(),
                context.acquire_response_digest.as_deref(),
                context.engine_id.as_str(),
                context.docker_root_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO physical_attempt_current (lineage_id, generation, reservation_id, intent_id) VALUES (?1, ?2, ?3, ?4)",
            (lineage_id, generation, reservation_id, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(PhysicalAttemptIdentity {
        lineage_id,
        generation,
        reservation_id,
        intent_id,
        witness_request_id,
        physical_nonce: physical_nonce.to_owned(),
        binding_digest: scope.binding_digest().to_owned(),
        source_digest: context.source_digest,
        acquire_response_digest: context.acquire_response_digest,
        engine_id: context.engine_id,
        docker_root_digest: context.docker_root_digest,
    })
}

struct AttemptContext {
    scale_set_id: i64,
    source_digest: String,
    acquire_response_digest: Option<String>,
    engine_id: String,
    docker_root_digest: String,
}

async fn attempt_context(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    intent_id: i64,
    witness_request_id: i64,
    nonce: &str,
) -> Result<AttemptContext, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent.scale_set_id, auth.source_digest, auth.acquire_response_digest, reservation.engine_id, reservation.docker_root_digest FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id JOIN connection_request_authorizations AS auth ON auth.connection_id = reservation.connection_id AND auth.scale_set_id = intent.scale_set_id AND auth.request_id = ?1 WHERE reservation.id = ?2 AND reservation.intent_id = ?3 AND reservation.connection_id = ?4 AND reservation.binding_digest = ?5 AND reservation.state = 'reserved' AND intent.kind = 'launch' AND intent.state = 'pending' AND intent.scale_set_id = ?6 AND intent.runner_name = 'v' || ?7 AND intent.runner_request_id IS NULL AND auth.binding_digest = ?5 AND auth.acquire_state IN ('requested', 'acquired') AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL",
            (
                witness_request_id,
                reservation_id,
                intent_id,
                scope.connection_id().as_str(),
                scope.binding_digest(),
                scale_set_id(connection, scope).await?,
                nonce,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let context = AttemptContext {
        scale_set_id: row.get(0).map_err(|_| HostError::Journal)?,
        source_digest: row.get(1).map_err(|_| HostError::Journal)?,
        acquire_response_digest: row.get(2).map_err(|_| HostError::Journal)?,
        engine_id: row.get(3).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(4).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(context)
}

async fn scale_set_id(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [scope.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn reusable_lineage(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
) -> Result<Option<(i64, i64)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT lineage.lineage_id, MAX(attempt.generation) + 1 FROM physical_attempt_lineages AS lineage JOIN physical_attempts AS attempt ON attempt.lineage_id = lineage.lineage_id JOIN physical_attempt_outcomes AS outcome ON outcome.lineage_id = attempt.lineage_id AND outcome.generation = attempt.generation AND outcome.reservation_id = attempt.reservation_id AND outcome.outcome IN ('pre_jit_capacity_available', 'normal_completion') WHERE lineage.connection_id = ?1 AND lineage.scale_set_id = (SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1) AND lineage.binding_digest = ?2 AND NOT EXISTS (SELECT 1 FROM physical_attempt_current AS current WHERE current.lineage_id = lineage.lineage_id) GROUP BY lineage.lineage_id ORDER BY MAX(attempt.generation) DESC, lineage.lineage_id LIMIT 1",
            (scope.connection_id().as_str(), scope.binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let reusable = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(reusable))
}

async fn create_lineage(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
) -> Result<i64, HostError> {
    let scale_set_id = scale_set_id(connection, scope).await?;
    connection
        .execute(
            "INSERT INTO physical_attempt_lineages (connection_id, scale_set_id, binding_digest) VALUES (?1, ?2, ?3)",
            (scope.connection_id().as_str(), scale_set_id, scope.binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let lineage_id = connection.last_insert_rowid();
    if lineage_id <= 0 {
        return Err(HostError::Journal);
    }
    Ok(lineage_id)
}
