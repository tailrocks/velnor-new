use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::BootstrapStartReceipt;

use super::super::storage_plan::load as load_storage_plan;
use super::super::{hold_context, hold_is_claimable, mark_hold_uncertain};
use super::BootstrapStartClaim;

impl ConnectionJournal {
    /// After all volume and container creation operations complete, record permission to start both bootstrap containers.
    pub(crate) async fn claim_bootstrap_start(
        &self,
        reservation_id: i64,
    ) -> Result<Option<BootstrapStartClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) =
                hold_context(connection, &connection_id, reservation_id).await?
            else {
                return Ok(None);
            };
            if !hold_is_claimable(&context)
                || !super::objects_are_created(connection, reservation_id).await?
                || !super::super::initialization::ready_for_job_create(connection, reservation_id).await?
            {
                return Ok(None);
            }
            let _ = load_storage_plan(connection, reservation_id, &context).await?;
            let Some((runner_container_id, dind_container_id)) =
                created_container_ids(connection, reservation_id).await?
            else {
                return Ok(None);
            };
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET start_state = 'requested' WHERE reservation_id = ?1 AND create_state = 'created' AND start_state = 'not_requested'",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 2 {
                return Ok(None);
            }
            let state_changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'requested' WHERE reservation_id = ?1 AND bootstrap_start_state = 'not_requested' AND runner_container_id IS NULL AND dind_container_id IS NULL",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if state_changed != 1 {
                return Err(HostError::Journal);
            }
            Ok(Some(BootstrapStartClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                runner_container_id,
                dind_container_id,
            }))
        })
        .await
    }

    /// Record that both bootstrap containers run with Runner stdin open and JIT withheld.
    pub(crate) async fn record_bootstrap_started(
        &self,
        claim: &BootstrapStartClaim,
        proof: &BootstrapStartReceipt,
    ) -> Result<(), HostError> {
        validate_start_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.runner_container_id() != claim.runner_container_id
            || proof.dind_container_id() != claim.dind_container_id
            || !proof.runner_stdin_open()
            || !proof.jit_withheld()
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(
                connection,
                &claim.connection_id,
                claim.reservation_id,
            )
            .await?
            else {
                return Err(HostError::Journal);
            };
            if context.operation_id != claim.operation_id
                || context.reservation_state != "reserved"
                || context.intent_state != "pending"
            {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET start_state = 'started' WHERE reservation_id = ?1 AND start_state = 'requested' AND create_state = 'created'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 2 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'started', runner_container_id = ?1, dind_container_id = ?2 WHERE reservation_id = ?3 AND bootstrap_start_state = 'requested' AND runner_container_id IS NULL AND dind_container_id IS NULL",
                    (claim.runner_container_id.as_str(), claim.dind_container_id.as_str(), claim.reservation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET docker_id = ?1, dind_id = ?2, launch_phase = 'bootstrap_started' WHERE id = ?3 AND connection_id = ?4 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'provisioning_held' AND docker_id IS NULL AND dind_id IS NULL AND EXISTS (SELECT 1 FROM capacity_reservations WHERE id = ?5 AND intent_id = ?3 AND connection_id = ?4 AND state = 'reserved')",
                    (claim.runner_container_id.as_str(), claim.dind_container_id.as_str(), claim.operation_id, claim.connection_id.as_str(), claim.reservation_id),
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

    /// Keep the full capacity reservation after a partial or uncertain bootstrap start.
    pub(crate) async fn mark_bootstrap_start_uncertain(
        &self,
        claim: &BootstrapStartClaim,
    ) -> Result<(), HostError> {
        validate_start_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET start_state = 'uncertain' WHERE reservation_id = ?1 AND create_state = 'created' AND start_state = 'requested'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 2 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state = 'uncertain' WHERE reservation_id = ?1 AND bootstrap_start_state = 'requested'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            mark_hold_uncertain(
                connection,
                &claim.connection_id,
                claim.reservation_id,
                claim.operation_id,
            )
            .await
        })
        .await
    }
}

async fn created_container_ids(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<(String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, container_id, create_state, start_state FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let (mut runner, mut dind) = (None, None);
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role: String = row.get(0).map_err(|_| HostError::Journal)?;
        let id: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
        let create_state: String = row.get(2).map_err(|_| HostError::Journal)?;
        let start_state: String = row.get(3).map_err(|_| HostError::Journal)?;
        if create_state != "created" || start_state != "not_requested" {
            return Ok(None);
        }
        match role.as_str() {
            "runner" => runner = id,
            "dind" => dind = id,
            _ => return Err(HostError::Journal),
        }
    }
    let Some(runner) = runner.filter(|id| !super::invalid_resource_id(id)) else {
        return Ok(None);
    };
    let Some(dind) = dind.filter(|id| !super::invalid_resource_id(id)) else {
        return Ok(None);
    };
    Ok(Some((runner, dind)))
}

fn validate_start_claim(
    journal: &ConnectionJournal,
    claim: &BootstrapStartClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || super::invalid_resource_id(&claim.runner_container_id)
        || super::invalid_resource_id(&claim.dind_container_id)
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}
