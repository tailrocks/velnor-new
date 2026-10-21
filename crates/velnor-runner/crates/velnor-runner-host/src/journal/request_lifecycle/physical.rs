//! One-shot JIT claims for physical runner capacity rows.

use super::RunnerJitClaim;
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
#[cfg(test)]
use crate::journal::connection::storage::read_transaction;
use crate::journal::source_authorization::created_set_matches;
use velnor_runner_github::RunnerReference;

use super::helpers::bound_set_id;
use super::set_scope::set_is_exclusive;
mod support;
pub(in crate::journal) use support::mark_runner_cleaned_after_cleanup;
use support::{
    has_legacy_receipts, has_uncertain_acquire, physical_intent_is_fresh, valid_runner_name,
    validate_jit_claim,
};

impl ConnectionJournal {
    #[cfg(test)]
    pub(crate) async fn test_active_authority_identity(&self) -> Result<(u64, String), HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            let mut rows = connection
                .query(
                    "SELECT active.generation, active.policy_digest FROM connection_active_job_authority AS active JOIN connection_job_authority_policies AS policy ON policy.connection_id = active.connection_id AND policy.generation = active.generation AND policy.policy_digest = active.policy_digest AND policy.binding_digest = ?2 WHERE active.connection_id = ?1",
                    (self.connection_id().as_str(), self.binding_digest()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows.next().await.map_err(|_| HostError::Journal)?;
            let Some(row) = row else {
                return Err(HostError::Identity);
            };
            let generation: i64 = row.get(0).map_err(|_| HostError::Journal)?;
            let digest: String = row.get(1).map_err(|_| HostError::Journal)?;
            if generation <= 0
                || digest.len() != 64
                || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            {
                return Err(HostError::Journal);
            }
            Ok((u64::try_from(generation).map_err(|_| HostError::Journal)?, digest))
        })
        .await
    }

    /// Record JIT permission for the specified acquired request and its reservation.
    pub(crate) async fn claim_runner_jit_for_request(
        &self,
        intent_id: i64,
        reservation_id: i64,
        request_id: i64,
        requested_name: &str,
        policy_generation: u64,
        policy_digest: &str,
    ) -> Result<Option<RunnerJitClaim>, HostError> {
        if intent_id <= 0
            || reservation_id <= 0
            || request_id <= 0
            || policy_generation == 0
            || policy_digest.len() != 64
            || !policy_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !valid_runner_name(requested_name)
        {
            return Err(HostError::Identity);
        }
        let policy_generation =
            i64::try_from(policy_generation).map_err(|_| HostError::Identity)?;
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        let requested_name = requested_name.to_owned();
        let policy_digest = policy_digest.to_owned();
        let result = in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let set_id = bound_set_id(connection, &connection_id).await?;
            if crate::journal::connection::has_unresolved_legacy_work(connection).await?
                || !set_is_exclusive(connection, &connection_id, set_id).await?
                || !created_set_matches(connection, &connection_id, set_id, &binding_digest).await?
                || !active_policy_matches(connection, &connection_id, &binding_digest, policy_generation, &policy_digest).await?
                || has_legacy_receipts(connection, &connection_id, set_id).await?
                || has_uncertain_acquire(connection, &connection_id, set_id).await?
                || !super::acquire::acquired_backlog_is_closed(connection, &connection_id, set_id, &binding_digest).await?
            {
                return Ok(None);
            }
            let mut intent_rows = connection.query(
                "SELECT launch_phase FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 0 AND runner_name = ?4",
                (intent_id, connection_id.as_str(), set_id, requested_name.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(intent_row) = intent_rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None) };
            let phase: String = intent_row.get(0).map_err(|_| HostError::Journal)?;
            if intent_rows.next().await.map_err(|_| HostError::Journal)?.is_some() { return Err(HostError::Journal); }
            if phase != "resources_bound"
                || !physical_intent_is_fresh(connection, &connection_id, set_id, intent_id, &requested_name).await?
                || !crate::journal::connection::validate_resources_bound(
                    connection,
                    &connection_id,
                    intent_id,
                )
                .await?
            { return Ok(None); }
            let mut rows = connection.query(
                "SELECT auth.request_id FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id AND auth.binding_digest = attempt.binding_digest WHERE attempt.connection_id = ?1 AND attempt.scale_set_id = ?2 AND attempt.witness_request_id = ?3 AND current.reservation_id = ?4 AND attempt.binding_digest = ?5 AND current.intent_id = ?6 AND auth.acquire_state = 'acquired' AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND EXISTS (SELECT 1 FROM capacity_reservations AS r WHERE r.id = current.reservation_id AND r.intent_id = ?6 AND r.connection_id = attempt.connection_id AND r.state = 'reserved')",
                (connection_id.as_str(), set_id, request_id, reservation_id, binding_digest.as_str(), intent_id),
            ).await.map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None) };
            let witness_request_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || witness_request_id != request_id {
                return Err(HostError::Journal);
            }
            connection.execute(
                "INSERT INTO connection_runner_jits (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state) VALUES (?1, ?2, ?3, ?4, ?5, 'claimed')",
                (intent_id, connection_id.as_str(), set_id, request_id, requested_name.as_str()),
            ).await.map_err(|_| HostError::Journal)?;
            let changed = connection.execute("UPDATE connection_intents SET jit_requested = 1, launch_phase = 'jit_requested' WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 0 AND acquire_attempted = 0 AND runner_name = ?4 AND launch_phase = 'resources_bound' AND worker_volume IS NULL AND docker_id = (SELECT s.runner_container_id FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.id = ?5 AND r.intent_id = ?1 AND r.connection_id = ?2 AND r.state = 'reserved' AND s.resources_bound = 1) AND dind_id = (SELECT s.dind_container_id FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.id = ?5 AND r.intent_id = ?1 AND r.connection_id = ?2 AND r.state = 'reserved' AND s.resources_bound = 1)", (intent_id, connection_id.as_str(), set_id, requested_name.as_str(), reservation_id)).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            let changed = connection.execute("UPDATE capacity_reservation_provisioning_state SET jit_state = 'requested' WHERE reservation_id = ?1 AND resources_bound = 1 AND bootstrap_start_state = 'started' AND jit_state = 'not_requested' AND publication_state = 'not_requested' AND delivery_state = 'not_requested' AND storage_cleanup_proven = 0", [reservation_id]).await.map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            Ok(Some((set_id, request_id)))
        }).await?;
        let Some((scale_set_id, witness_request_id)) = result else {
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
                    "UPDATE connection_intents SET github_runner_id = ?1, launch_phase = 'jit_bound' WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 1 AND github_runner_id IS NULL AND runner_name = ?2 AND launch_phase = 'jit_requested' AND worker_volume IS NULL AND docker_id = (SELECT s.runner_container_id FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.intent_id = ?3 AND r.connection_id = ?4 AND r.state = 'reserved' AND s.resources_bound = 1) AND dind_id = (SELECT s.dind_container_id FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.intent_id = ?3 AND r.connection_id = ?4 AND r.state = 'reserved' AND s.resources_bound = 1)",
                    (runner.id.to_string(), runner.name.as_str(), claim.intent_id, self.connection_id().as_str(), claim.scale_set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET jit_state = 'bound', runner_id = ?1, runner_name = ?2, runner_scale_set_id = ?3 WHERE reservation_id = (SELECT id FROM capacity_reservations WHERE intent_id = ?4 AND connection_id = ?5 AND state = 'reserved') AND jit_state = 'requested' AND resources_bound = 1 AND publication_state = 'not_requested' AND delivery_state = 'not_requested' AND runner_id IS NULL AND runner_name IS NULL AND runner_scale_set_id IS NULL",
                    (runner.id, runner.name.as_str(), runner.runner_scale_set_id, claim.intent_id, self.connection_id().as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
        })
        .await
    }

    /// Keep an interrupted JIT operation claim and its capacity reservation unavailable for replay.
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
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET jit_state = 'uncertain' WHERE reservation_id = (SELECT id FROM capacity_reservations WHERE intent_id = ?1 AND connection_id = ?2 AND state = 'reserved') AND jit_state = 'requested' AND resources_bound = 1",
                    (claim.intent_id, self.connection_id().as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservations SET state = 'uncertain' WHERE intent_id = ?1 AND connection_id = ?2 AND state = 'reserved'",
                    (claim.intent_id, self.connection_id().as_str()),
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
}

async fn active_policy_matches(
    connection: &turso::Connection,
    connection_id: &str,
    binding_digest: &str,
    generation: i64,
    policy_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM connection_active_job_authority AS active JOIN connection_job_authority_policies AS policy ON policy.connection_id = active.connection_id AND policy.generation = active.generation AND policy.policy_digest = active.policy_digest WHERE active.connection_id = ?1 AND active.generation = ?2 AND active.policy_digest = ?3 AND policy.binding_digest = ?4",
            (connection_id, generation, policy_digest, binding_digest),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}
