// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    UnboundContainer, UnboundRetirementClaim, UnboundVolume, inventory,
    validate_claim_in_transaction,
};
use crate::error::HostError;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::{ConnectionJournal, UnboundCreationObservation};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnboundResourceKind {
    Container,
    Volume,
    Network,
}

impl UnboundResourceKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Volume => "volume",
            Self::Network => "network",
        }
    }
}

pub(crate) struct UnboundRetirementInventory {
    pub(crate) containers: Vec<UnboundContainer>,
    pub(crate) volumes: Vec<UnboundVolume>,
}

impl ConnectionJournal {
    pub(crate) async fn record_unbound_creation_target(
        &self,
        claim: &UnboundRetirementClaim,
        observation: &UnboundCreationObservation,
    ) -> Result<(), HostError> {
        self.validate_claim_identity(claim)?;
        if observation.reservation_id() != claim.reservation_id
            || observation.connection_id() != claim.connection_id
            || observation.operation_id() != claim.operation_id
            || observation.engine_id() != claim.engine_id
            || observation.docker_root_digest() != claim.docker_root_digest
        {
            return Err(HostError::Ownership);
        }
        crate::journal::unbound_observation::validate(
            observation.observation_json(),
            observation.observation_sha256(),
        )?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            let existing = target(connection, claim, observation.resource_kind(), observation.role()).await?;
            if let Some(id) = existing {
                return if id==observation.docker_id() { Ok(()) } else { Err(HostError::Ownership) };
            }
            connection.execute("INSERT INTO capacity_reservation_unbound_creation_targets(reservation_id,resource_kind,role,docker_name,docker_id,observation_sha256,observation_json,connection_id,operation_id,engine_id,docker_root_digest,observation_claim_generation,observed_at_unix_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,unixepoch()*1000)", (claim.reservation_id,observation.resource_kind().as_str(),observation.role(),observation.docker_name(),observation.docker_id(),observation.observation_sha256(),observation.observation_json(),claim.connection_id.as_str(),claim.operation_id,claim.engine_id.as_str(),claim.docker_root_digest.as_str(),claim.generation.load(Ordering::Acquire))).await.map_err(|_| HostError::Journal)?;
            Ok(())
        }).await
    }

    pub(crate) async fn unbound_creation_target(
        &self,
        claim: &UnboundRetirementClaim,
        kind: UnboundResourceKind,
        role: &str,
    ) -> Result<Option<String>, HostError> {
        self.validate_claim_identity(claim)?;
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            target(connection, claim, kind, role).await
        })
        .await
    }

    pub(crate) async fn refresh_unbound_retirement_inventory(
        &self,
        claim: &UnboundRetirementClaim,
    ) -> Result<UnboundRetirementInventory, HostError> {
        self.validate_claim_identity(claim)?;
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            Ok(UnboundRetirementInventory {
                containers: inventory::containers(connection, claim.reservation_id).await?,
                volumes: inventory::volumes(connection, claim.reservation_id).await?,
            })
        })
        .await
    }

    pub(crate) async fn defer_unbound_retirement_claim(
        &self,
        claim: &UnboundRetirementClaim,
    ) -> Result<(), HostError> {
        self.validate_claim_identity(claim)?;
        let generation = claim.generation.load(Ordering::Acquire);
        let next = generation.checked_add(1).ok_or(HostError::Journal)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            let changed=connection.execute("UPDATE capacity_reservation_unbound_retirement_claims SET claim_generation=?1,lease_until=1 WHERE reservation_id=?2 AND claim_generation=?3 AND lease_until=?4", (next,claim.reservation_id,generation,claim.lease_until.load(Ordering::Acquire))).await.map_err(|_| HostError::Journal)?;
            if changed!=1 { return Err(HostError::Ownership); }
            Ok(())
        }).await?;
        claim.generation.store(next, Ordering::Release);
        claim.lease_until.store(1, Ordering::Release);
        Ok(())
    }
}

async fn target(
    connection: &turso::Connection,
    claim: &UnboundRetirementClaim,
    kind: UnboundResourceKind,
    role: &str,
) -> Result<Option<String>, HostError> {
    let mut rows=connection.query("SELECT docker_id FROM capacity_reservation_unbound_creation_targets WHERE reservation_id=?1 AND resource_kind=?2 AND role=?3 AND connection_id=?4 AND operation_id=?5 AND engine_id=?6 AND docker_root_digest=?7", (claim.reservation_id,kind.as_str(),role,claim.connection_id.as_str(),claim.operation_id,claim.engine_id.as_str(),claim.docker_root_digest.as_str())).await.map_err(|_|HostError::Journal)?;
    let id = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .map(|row| row.get(0).map_err(|_| HostError::Journal))
        .transpose()?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(id)
}
