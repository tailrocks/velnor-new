use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::ListenerDeliveryReceipt;

use super::super::{hold_context, hold_is_claimable, mark_hold_uncertain};
use super::ListenerDeliveryClaim;
use super::validation::{
    bound_state, jit_runner, same_runner, validate_delivery_claim, validate_delivery_state,
};

impl ConnectionJournal {
    /// Record permission to write the recorded JIT data once to the open runner stdin.
    pub(crate) async fn claim_listener_delivery(
        &self,
        reservation_id: i64,
    ) -> Result<Option<ListenerDeliveryClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !hold_is_claimable(&context) { return Ok(None); }
            let Some(record) = bound_state(connection, reservation_id).await? else { return Ok(None); };
            if !record.resources_bound || record.jit_state != "bound" || record.publication_state != "published" || record.delivery_state != "not_requested" || record.cleanup_proven {
                return Ok(None);
            }
            let runner = jit_runner(connection, &connection_id, context.operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET delivery_state = 'requested' WHERE reservation_id = ?1 AND resources_bound = 1 AND jit_state = 'bound' AND publication_state = 'published' AND delivery_state = 'not_requested' AND storage_cleanup_proven = 0",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Ok(None); }
            Ok(Some(ListenerDeliveryClaim {
                connection_id: connection_id.clone(), reservation_id,
                operation_id: context.operation_id, engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                runner_container_id: record.runner_container_id,
                dind_container_id: record.dind_container_id,
                instance_binding_digest: record.instance_binding_digest,
                runner,
            }))
        })
        .await
    }

    /// Record that the exact JIT bytes reached the held Runner stdin and EOF closed it.
    pub(crate) async fn record_listener_delivered(
        &self,
        claim: &ListenerDeliveryClaim,
        proof: &ListenerDeliveryReceipt,
    ) -> Result<(), HostError> {
        validate_delivery_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.runner_container_id() != claim.runner_container_id
            || !same_runner(proof.runner(), &claim.runner)
            || !proof.delivered()
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_delivery_state(connection, claim).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET delivery_state = 'delivered' WHERE reservation_id = ?1 AND delivery_state = 'requested' AND publication_state = 'published' AND jit_state = 'bound' AND resources_bound = 1",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            let changed = connection
                .execute(
                    "UPDATE connection_runner_jits SET state = 'ready' WHERE intent_id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND state = 'bound' AND runner_id = ?4 AND runner_name = ?5",
                    (claim.operation_id, claim.connection_id.as_str(), claim.runner.runner_scale_set_id, claim.runner.id, claim.runner.name.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET launch_phase = 'worker_ready' WHERE id = ?1 AND connection_id = ?2 AND scale_set_id = ?3 AND state = 'pending' AND cleanup_proven = 0 AND github_runner_id = ?4 AND runner_name = ?5 AND docker_id = ?6 AND dind_id = ?7 AND launch_phase = 'job_facts_published'",
                    (claim.operation_id, claim.connection_id.as_str(), claim.runner.runner_scale_set_id, claim.runner.id.to_string(), claim.runner.name.as_str(), claim.runner_container_id.as_str(), claim.dind_container_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
        })
        .await
    }

    /// Keep capacity charged when only part of the JIT payload may have been written.
    pub(crate) async fn mark_listener_delivery_uncertain(
        &self,
        claim: &ListenerDeliveryClaim,
    ) -> Result<(), HostError> {
        validate_delivery_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET delivery_state = 'uncertain' WHERE reservation_id = ?1 AND delivery_state = 'requested' AND publication_state = 'published' AND jit_state = 'bound'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            mark_hold_uncertain(connection, &claim.connection_id, claim.reservation_id, claim.operation_id).await
        })
        .await
    }
}
