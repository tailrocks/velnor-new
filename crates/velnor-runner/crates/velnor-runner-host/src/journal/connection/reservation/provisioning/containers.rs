use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::ConnectionJournal;
use crate::launch::bootstrap::{
    BootstrapContainerRole, BootstrapStartReceipt, VerifiedBootstrapContainerCreated,
};

use super::admission::container_role;
use super::{hold_context, hold_is_claimable, mark_hold_uncertain};

/// One durable container create attempt.
pub(crate) struct BootstrapContainerCreateClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: BootstrapContainerRole,
    container_name: String,
}

impl BootstrapContainerCreateClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) const fn role(&self) -> BootstrapContainerRole {
        self.role
    }

    pub(crate) fn container_name(&self) -> &str {
        &self.container_name
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
}

/// One atomic claim before starting the JIT-withheld container pair.
pub(crate) struct BootstrapStartClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
}

impl BootstrapStartClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
}

impl ConnectionJournal {
    /// Claim one JIT-free container create before sending Docker the request.
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
                || !objects_are_created(connection, reservation_id).await?
            {
                return Ok(None);
            }
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
            || invalid_resource_id(proof.container_id())
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

    /// Keep the full hold after an uncertain Docker container create.
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

    /// Claim both bootstrap starts after every volume and container create settled.
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
            if !hold_is_claimable(&context) || !objects_are_created(connection, reservation_id).await? {
                return Ok(None);
            }
            let mut rows = connection
                .query(
                    "SELECT role, container_id, create_state, start_state FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 ORDER BY role",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut runner = None;
            let mut dind = None;
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
            let Some(runner_container_id) = runner.filter(|id| !invalid_resource_id(id)) else {
                return Ok(None);
            };
            let Some(dind_container_id) = dind.filter(|id| !invalid_resource_id(id)) else {
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
                    "UPDATE connection_intents SET launch_phase = 'bootstrap_started' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'provisioning_held'",
                    (claim.operation_id, claim.connection_id.as_str()),
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

    /// Preserve the hold after a partial or uncertain bootstrap start.
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

async fn objects_are_created(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND create_state = 'created' AND docker_id = docker_name",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 4)
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

fn validate_start_claim(
    journal: &ConnectionJournal,
    claim: &BootstrapStartClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || invalid_resource_id(&claim.runner_container_id)
        || invalid_resource_id(&claim.dind_container_id)
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

fn invalid_resource_id(value: &str) -> bool {
    value.is_empty()
        || value.len() > 256
        || value.bytes().any(|byte| byte.is_ascii_control())
}

