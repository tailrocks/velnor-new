//! One-shot JIT and Docker start claims for physical runner capacity rows.

use super::{PhysicalRunnerStartClaim, RunnerJitClaim};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::source_authorization::created_set_matches;
use crate::journal::sql::token_rejected;
use velnor_runner_github::RunnerReference;

use super::helpers::bound_set_id;
use super::set_scope::set_is_exclusive;

impl ConnectionJournal {
    /// Claim one runner JIT and choose an acquired request as its audit witness.
    pub(crate) async fn claim_runner_jit(
        &self,
        intent_id: i64,
        requested_name: &str,
    ) -> Result<Option<RunnerJitClaim>, HostError> {
        if intent_id <= 0 || !valid_runner_name(requested_name) {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        let requested_name = requested_name.to_owned();
        let witness = in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let set_id = bound_set_id(connection, &connection_id).await?;
            if crate::journal::connection::has_unresolved_legacy_work(connection).await?
                || !set_is_exclusive(connection, &connection_id, set_id).await?
                || !created_set_matches(connection, &connection_id, set_id, &binding_digest).await?
                || has_legacy_receipts(connection, &connection_id, set_id).await?
                || has_uncertain_acquire(connection, &connection_id, set_id).await?
                || !super::acquire::acquired_backlog_is_closed(
                    connection,
                    &connection_id,
                    set_id,
                    &binding_digest,
                )
                .await?
                || !physical_intent_is_fresh(
                    connection,
                    &connection_id,
                    set_id,
                    intent_id,
                    &requested_name,
                )
                .await?
            {
                return Ok(None);
            }
            let Some(witness_request_id) = acquired_witness(connection, &connection_id, set_id, &binding_digest).await? else {
                return Ok(None);
            };
            let inserted = connection
                .execute(
                    "INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state) VALUES (?1, ?2, ?3, ?4, ?5, 'claimed')",
                    (intent_id, connection_id.as_str(), set_id, witness_request_id, requested_name.as_str()),
                )
                .await;
            if inserted.is_err() {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET jit_requested = 1, launch_phase = 'jit_requested' WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 0 AND acquire_attempted = 0 AND docker_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL",
                    (intent_id, connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            Ok(Some((set_id, witness_request_id)))
        })
        .await?;
        let Some((scale_set_id, witness_request_id)) = witness else {
            return Ok(None);
        };
        Ok(Some(RunnerJitClaim {
            connection_id: self.connection_id().clone(),
            scale_set_id,
            intent_id,
            witness_request_id,
            requested_name,
        }))
    }

    /// Record the exact JIT-created GitHub runner before any Docker effect.
    pub(crate) async fn bind_jit_runner(
        &self,
        claim: RunnerJitClaim,
        runner: &RunnerReference,
    ) -> Result<(), HostError> {
        validate_jit_claim(self, &claim)?;
        if runner.id <= 0
            || runner.runner_scale_set_id != claim.scale_set_id
            || runner.name != claim.requested_name
            || !valid_runner_name(&runner.name)
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_runner_jits SET state = 'bound', runner_id = ?1, runner_name = ?2 WHERE intent_id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND witness_request_id = ?6 AND requested_name = ?2 AND state = 'claimed' AND runner_id IS NULL AND runner_name IS NULL",
                    (runner.id, runner.name.as_str(), claim.intent_id, self.connection_id().as_str(), claim.scale_set_id, claim.witness_request_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET github_runner_id = ?1, runner_name = ?2, launch_phase = 'jit_bound' WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 1 AND github_runner_id IS NULL AND runner_name IS NULL AND docker_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL",
                    (runner.id.to_string(), runner.name.as_str(), claim.intent_id, self.connection_id().as_str(), claim.scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Keep an interrupted JIT claim charged and unavailable for replay.
    pub(crate) async fn mark_runner_jit_uncertain(
        &self,
        claim: RunnerJitClaim,
    ) -> Result<(), HostError> {
        validate_jit_claim(self, &claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_runner_jits SET state = 'uncertain' WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND witness_request_id = ?4 AND requested_name = ?5 AND state = 'claimed' AND runner_id IS NULL AND runner_name IS NULL",
                    (claim.intent_id, self.connection_id().as_str(), claim.scale_set_id, claim.witness_request_id, claim.requested_name.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "UPDATE connection_intents SET state = 'uncertain', launch_phase = 'jit_uncertain' WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND jit_requested = 1",
                    (claim.intent_id, self.connection_id().as_str(), claim.scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Persist the unique worker volume and one-shot Docker start claim.
    pub(crate) async fn claim_physical_runner_start(
        &self,
        intent_id: i64,
        worker_volume: &str,
    ) -> Result<Option<PhysicalRunnerStartClaim>, HostError> {
        if intent_id <= 0 || !valid_worker_volume(worker_volume) {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let worker_volume = worker_volume.to_owned();
        let scale_set_id = in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let set_id = bound_set_id(connection, &connection_id).await?;
            let ready = connection
                .execute(
                    "UPDATE connection_runner_jits SET state = 'start_requested' WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND state = 'bound' AND runner_id IS NOT NULL AND runner_name IS NOT NULL",
                    (intent_id, connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if ready != 1 {
                return Ok(None);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET worker_volume = ?1, launch_phase = 'docker_requested' WHERE id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'jit_bound' AND worker_volume IS NULL AND docker_id IS NULL AND dind_id IS NULL AND NOT EXISTS (SELECT 1 FROM connection_intents WHERE worker_volume = ?1)",
                    (worker_volume.as_str(), intent_id, connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(Some(set_id))
            } else {
                Err(HostError::Journal)
            }
        })
        .await?;
        let Some(scale_set_id) = scale_set_id else {
            return Ok(None);
        };
        Ok(Some(PhysicalRunnerStartClaim {
            connection_id: self.connection_id().clone(),
            scale_set_id,
            intent_id,
            worker_volume,
        }))
    }

    /// Mark a Docker start with no complete response uncertain and keep its charge.
    pub(crate) async fn mark_physical_runner_start_uncertain(
        &self,
        claim: PhysicalRunnerStartClaim,
    ) -> Result<(), HostError> {
        validate_start_claim(self, &claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_runner_jits SET state = 'start_uncertain' WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND state = 'start_requested'",
                    (claim.intent_id, self.connection_id().as_str(), claim.scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "UPDATE connection_intents SET state = 'uncertain', launch_phase = 'docker_uncertain' WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND worker_volume = ?4 AND cleanup_proven = 0",
                    (claim.intent_id, self.connection_id().as_str(), claim.scale_set_id, claim.worker_volume.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Bind a fully created and inspected worker pair to its one-shot start claim.
    pub(crate) async fn record_physical_worker_ready(
        &self,
        claim: PhysicalRunnerStartClaim,
        runner_container_id: &str,
        dind_container_id: &str,
    ) -> Result<(), HostError> {
        validate_start_claim(self, &claim)?;
        if !valid_resource_id(runner_container_id) || !valid_resource_id(dind_container_id) {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET docker_id = ?1, dind_id = ?2, launch_phase = 'worker_ready' WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND worker_volume = ?6 AND launch_phase = 'docker_requested' AND docker_id IS NULL AND dind_id IS NULL",
                    (runner_container_id, dind_container_id, claim.intent_id, self.connection_id().as_str(), claim.scale_set_id, claim.worker_volume.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_runner_jits SET state = 'ready' WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND state = 'start_requested' AND runner_id IS NOT NULL",
                    (claim.intent_id, self.connection_id().as_str(), claim.scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }
}

fn validate_jit_claim(scope: &ConnectionJournal, claim: &RunnerJitClaim) -> Result<(), HostError> {
    if &claim.connection_id != scope.connection_id()
        || claim.scale_set_id <= 0
        || claim.intent_id <= 0
        || claim.witness_request_id <= 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_start_claim(
    scope: &ConnectionJournal,
    claim: &PhysicalRunnerStartClaim,
) -> Result<(), HostError> {
    if &claim.connection_id != scope.connection_id()
        || claim.scale_set_id <= 0
        || claim.intent_id <= 0
        || !valid_worker_volume(&claim.worker_volume)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn physical_intent_is_fresh(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    intent_id: i64,
    requested_name: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.assignment_key FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id AND r.connection_id = i.connection_id WHERE i.id = ?1 AND i.connection_id = ?2 AND i.scale_set_id = ?3 AND i.kind = 'launch' AND i.state = 'pending' AND i.cleanup_proven = 0 AND i.jit_requested = 0 AND i.acquire_attempted = 0 AND i.github_runner_id IS NULL AND i.runner_name IS NULL AND i.worker_volume IS NULL AND i.docker_id IS NULL AND i.dind_id IS NULL AND i.launch_phase IS NULL AND r.state = 'reserved' AND NOT EXISTS (SELECT 1 FROM connection_runner_jits WHERE intent_id = ?1) AND NOT EXISTS (SELECT 1 FROM connection_intents WHERE runner_name = ?4 AND cleanup_proven = 0) AND NOT EXISTS (SELECT 1 FROM connection_runner_jits WHERE requested_name = ?4 AND state != 'cleaned')",
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

async fn acquired_witness(
    connection: &turso::Connection,
    connection_id: &str,
    set_id: i64,
    binding_digest: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT request_id FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND binding_digest = ?3 AND authority_model = 'created_empty_exclusive_admin_session_v1' AND acquire_state = 'acquired' AND started_event_digest IS NULL AND completed_event_digest IS NULL AND NOT EXISTS (SELECT 1 FROM connection_runner_jits WHERE connection_runner_jits.connection_id = ?1 AND connection_runner_jits.scale_set_id = ?2 AND connection_runner_jits.witness_request_id = connection_request_authorizations.request_id AND connection_runner_jits.state != 'cleaned') ORDER BY request_id LIMIT 1",
            (connection_id, set_id, binding_digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .map(|row| row.get(0).map_err(|_| HostError::Journal))
        .transpose()
}

async fn has_legacy_receipts(
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

async fn has_uncertain_acquire(
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

fn valid_runner_name(value: &str) -> bool {
    !token_rejected(value) && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_worker_volume(value: &str) -> bool {
    value.strip_prefix('w').is_some_and(valid_nonce)
}

fn valid_resource_id(value: &str) -> bool {
    !token_rejected(value) && value.len() <= 256 && !value.chars().any(char::is_control)
}
