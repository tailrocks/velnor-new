use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::{BootstrapContainerRole, VerifiedBootstrapContainerCreated};

use super::super::admission::container_role;
use super::super::storage_plan::load as load_storage_plan;
use super::super::{hold_context, hold_is_claimable, mark_hold_uncertain};
use super::BootstrapContainerCreateClaim;
mod pre_jit_spec;
#[cfg(test)]
mod terminal_response;

impl ConnectionJournal {
    /// Record permission to create one container without JIT data before you send the Docker request.
    pub(crate) async fn claim_bootstrap_container_create(
        &self,
        reservation_id: i64,
        role: BootstrapContainerRole,
    ) -> Result<Option<BootstrapContainerCreateClaim>, HostError> {
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
            let storage_plan = load_storage_plan(connection, reservation_id, &context).await?;
            let role_text = container_role(role);
            let mut rows = connection
                .query(
                    "SELECT container_name FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 AND role = ?2 AND create_state = 'unclaimed' AND container_id IS NULL AND start_state = 'not_requested'",
                    (reservation_id, role_text),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let container_name: String = row.get(0).map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET create_state = 'requested' WHERE reservation_id = ?1 AND role = ?2 AND create_state = 'unclaimed' AND container_id IS NULL AND start_state = 'not_requested'",
                    (reservation_id, role_text),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Ok(None);
            }
            Ok(Some(BootstrapContainerCreateClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                role,
                container_name,
                storage_plan,
            }))
        })
        .await
    }

    /// Record the exact ID and inspected name for one created container.
    pub(crate) async fn record_bootstrap_container_created(
        &self,
        claim: &BootstrapContainerCreateClaim,
        proof: &VerifiedBootstrapContainerCreated,
    ) -> Result<(), HostError> {
        validate_create_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.role() != claim.role
            || proof.container_name() != claim.container_name
            || super::invalid_resource_id(proof.container_id())
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
                || context.engine_id != claim.engine_id
                || context.docker_root_digest != claim.docker_root_digest
                || context.reservation_state != "reserved"
                || context.intent_state != "pending"
            {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET create_state = 'created', container_id = ?1 WHERE reservation_id = ?2 AND role = ?3 AND container_name = ?4 AND create_state = 'requested' AND container_id IS NULL AND start_state = 'not_requested'",
                    (proof.container_id(), claim.reservation_id, container_role(claim.role), claim.container_name.as_str()),
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

    /// Record a completed daemon reply for the exact pending container request.
    /// This does not prove that Docker resources are absent.
    pub(crate) async fn record_bootstrap_container_create_terminal_response(
        &self,
        claim: &BootstrapContainerCreateClaim,
        status_code: u16,
        client_api_major: u16,
        client_api_minor: u16,
    ) -> Result<(), HostError> {
        validate_create_claim(self, claim)?;
        if !matches!(status_code, 400 | 404 | 409)
            || client_api_major != 1
            || client_api_minor != 53
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
            if !hold_is_claimable(&context)
                || context.operation_id != claim.operation_id
                || context.engine_id != claim.engine_id
                || context.docker_root_digest != claim.docker_root_digest
            {
                return Err(HostError::Journal);
            }
            let role = container_role(claim.role);
            let changed = connection
                .execute(
                    "INSERT INTO capacity_reservation_pre_jit_create_responses (reservation_id, role, connection_id, operation_id, engine_id, docker_root_digest, container_name, client_api_major, client_api_minor, response_status) SELECT ?1, ?2, ?3, ?4, ?5, ?6, container_name, ?7, ?8, ?9 FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 AND role = ?2 AND container_name = ?10 AND create_state = 'requested' AND container_id IS NULL AND start_state = 'not_requested'",
                    (claim.reservation_id, role, claim.connection_id.as_str(), claim.operation_id, claim.engine_id.as_str(), claim.docker_root_digest.as_str(), client_api_major, client_api_minor, status_code, claim.container_name.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET create_state = 'uncertain' WHERE reservation_id = ?1 AND role = ?2 AND container_name = ?3 AND create_state = 'requested' AND container_id IS NULL AND start_state = 'not_requested'",
                    (claim.reservation_id, role, claim.container_name.as_str()),
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

    /// Keep the full capacity reservation after an uncertain Docker container create.
    pub(crate) async fn mark_bootstrap_container_create_uncertain(
        &self,
        claim: &BootstrapContainerCreateClaim,
    ) -> Result<(), HostError> {
        validate_create_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_bootstrap_containers SET create_state = 'uncertain' WHERE reservation_id = ?1 AND role = ?2 AND container_name = ?3 AND create_state = 'requested' AND container_id IS NULL AND start_state = 'not_requested'",
                    (claim.reservation_id, container_role(claim.role), claim.container_name.as_str()),
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

fn validate_create_claim(
    journal: &ConnectionJournal,
    claim: &BootstrapContainerCreateClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || claim.container_name.is_empty()
        || claim.container_name.len() > 128
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}
