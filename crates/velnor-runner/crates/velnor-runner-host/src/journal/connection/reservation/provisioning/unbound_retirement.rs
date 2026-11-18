// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::schema::UNBOUND_SHAPE;

use super::hold_context;

#[cfg(test)]
#[path = "unbound_retirement/historical_v48.rs"]
mod historical_v48;

mod absence;
#[path = "unbound_retirement/inventory.rs"]
mod inventory;
#[cfg(test)]
#[path = "unbound_retirement/release_ready_tests.rs"]
mod release_ready_tests;
#[path = "unbound_retirement/ownership.rs"]
mod ownership;
#[path = "unbound_retirement/recovery.rs"]
mod recovery;
#[path = "unbound_retirement/settlement.rs"]
mod settlement;
#[path = "unbound_retirement/targets.rs"]
mod targets;
pub(super) use ownership::{owns_cleanup, reserves_cleanup};
use recovery::abandon_unbound_pairs;
pub(crate) use targets::{UnboundResourceKind, UnboundRetirementInventory};

use inventory::{current_claim, eligible, initializer};

const CLAIM_SECONDS: i64 = 45;

fn unbound_pair() -> String {
    UNBOUND_SHAPE.replace("{s}", "s")
}

const ABANDON_RESERVATIONS: &str = "UPDATE capacity_reservations SET state = 'uncertain' WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR connection_id = ?2) AND state = 'reserved' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0 AND EXISTS (SELECT 1 FROM connection_intents AS i JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = capacity_reservations.id WHERE i.id = capacity_reservations.intent_id AND i.connection_id = capacity_reservations.connection_id AND i.kind = 'launch' AND i.state = 'pending' AND i.cleanup_proven = 0 AND i.jit_requested = 0 AND {pair})";

const ABANDON_INTENTS: &str = "UPDATE connection_intents SET state = 'uncertain' WHERE kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND jit_requested = 0 AND EXISTS (SELECT 1 FROM capacity_reservations AS r JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.intent_id = connection_intents.id AND r.connection_id = connection_intents.connection_id AND (?1 IS NULL OR r.id = ?1) AND (?2 IS NULL OR r.connection_id = ?2) AND r.state = 'uncertain' AND r.remote_cleanup_proven = 0 AND r.local_cleanup_proven = 0 AND {pair})";

const UNCERTAIN_PAIRS: &str = "SELECT r.id FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id WHERE r.connection_id = ?1 AND r.state = 'uncertain' AND r.remote_cleanup_proven = 0 AND r.local_cleanup_proven = 0 AND i.kind = 'launch' AND i.state = 'uncertain' AND i.cleanup_proven = 0 AND {pair} ORDER BY r.id";

#[must_use]
#[derive(Clone, Debug)]
pub(crate) struct UnboundRetirementClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    generation: Arc<AtomicI64>,
    lease_until: Arc<AtomicI64>,
    initializer: Option<UnboundInitializer>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnboundContainer {
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) id: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnboundInitializer {
    pub(crate) name: String,
    pub(crate) id: Option<String>,
    pub(crate) operation_key: String,
}

#[derive(Clone, Debug)]
pub(crate) struct UnboundVolume {
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnboundRetirementPhase {
    ContainersAbsent,
    VolumesAbsent,
}

impl UnboundRetirementPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ContainersAbsent => "containers_absent",
            Self::VolumesAbsent => "volumes_absent",
        }
    }
}

impl UnboundRetirementClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn initializer(&self) -> Option<&UnboundInitializer> {
        self.initializer.as_ref()
    }
}

impl ConnectionJournal {
    pub(crate) async fn abandon_unbound_pair(&self, reservation_id: i64) -> Result<(), HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            abandon_unbound_pairs(connection, Some(reservation_id), Some(&connection_id)).await
        })
        .await
    }

    pub(crate) async fn uncertain_unbound_pairs(&self) -> Result<Vec<i64>, HostError> {
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    &UNCERTAIN_PAIRS.replace("{pair}", &unbound_pair()),
                    [connection_id.as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                ids.push(row.get(0).map_err(|_| HostError::Journal)?);
            }
            Ok(ids)
        })
        .await
    }

    pub(crate) async fn claim_unbound_retirement(
        &self,
        reservation_id: i64,
    ) -> Result<Option<UnboundRetirementClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let now = unix_seconds()?;
        let lease_until = now.checked_add(CLAIM_SECONDS).ok_or(HostError::Journal)?;
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await?
            else {
                return Ok(None);
            };
            if context.reservation_state != "uncertain"
                || context.intent_state != "uncertain"
                || context.cleanup_proven
                || context.topology_version != 2
                || !eligible(connection, &connection_id, reservation_id).await?
            {
                return Ok(None);
            }
            let generation = match current_claim(connection, reservation_id).await? {
                None => {
                    let inserted = connection.execute(
                        "INSERT INTO capacity_reservation_unbound_retirement_claims (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, claim_generation, lease_until) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                        (reservation_id, connection_id.as_str(), context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str(), lease_until),
                    ).await.map_err(|_| HostError::Journal)?;
                    if inserted != 1 {
                        return Err(HostError::Journal);
                    }
                    1
                }
                Some((_, old_lease)) if old_lease > now => return Ok(None),
                Some((old_generation, old_lease)) => {
                    let generation = old_generation.checked_add(1).ok_or(HostError::Journal)?;
                    let changed = connection.execute(
                        "UPDATE capacity_reservation_unbound_retirement_claims SET claim_generation = ?1, lease_until = ?2 WHERE reservation_id = ?3 AND connection_id = ?4 AND operation_id = ?5 AND claim_generation = ?6 AND lease_until = ?7",
                        (generation, lease_until, reservation_id, connection_id.as_str(), context.operation_id, old_generation, old_lease),
                    ).await.map_err(|_| HostError::Journal)?;
                    if changed != 1 {
                        return Ok(None);
                    }
                    generation
                }
            };
            Ok(Some(UnboundRetirementClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                generation: Arc::new(AtomicI64::new(generation)),
                lease_until: Arc::new(AtomicI64::new(lease_until)),
                initializer: initializer(connection, reservation_id).await?,
            }))
        })
        .await
    }

    pub(crate) async fn validate_unbound_retirement_claim(
        &self,
        claim: &UnboundRetirementClaim,
    ) -> Result<(), HostError> {
        self.validate_claim_identity(claim)?;
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await
        })
        .await
    }

    pub(crate) async fn renew_unbound_retirement_claim(
        &self,
        claim: &UnboundRetirementClaim,
    ) -> Result<(), HostError> {
        self.validate_claim_identity(claim)?;
        let now = unix_seconds()?;
        let renewed = now.checked_add(CLAIM_SECONDS).ok_or(HostError::Journal)?;
        let generation = claim.generation.load(Ordering::Acquire);
        let next = generation.checked_add(1).ok_or(HostError::Journal)?;
        let current = claim.lease_until.load(Ordering::Acquire);
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_unbound_retirement_claims SET claim_generation = ?1, lease_until = ?2 WHERE reservation_id = ?3 AND connection_id = ?4 AND operation_id = ?5 AND claim_generation = ?6 AND lease_until = ?7",
                    (next, renewed, claim.reservation_id, claim.connection_id.as_str(), claim.operation_id, generation, current),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Ownership) }
        })
        .await?;
        claim.generation.store(next, Ordering::Release);
        claim.lease_until.store(renewed, Ordering::Release);
        Ok(())
    }

    pub(crate) async fn unbound_retirement_phase_recorded(
        &self,
        claim: &UnboundRetirementClaim,
        phase: UnboundRetirementPhase,
    ) -> Result<bool, HostError> {
        self.validate_claim_identity(claim)?;
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT 1 FROM capacity_reservation_unbound_retirement_progress WHERE reservation_id = ?1 AND phase = ?2",
                    (claim.reservation_id, phase.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
        })
        .await
    }

    pub(crate) async fn record_unbound_retirement_phase(
        &self,
        claim: &UnboundRetirementClaim,
        phase: UnboundRetirementPhase,
        evidence_sha256: &str,
    ) -> Result<(), HostError> {
        self.validate_claim_identity(claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            let changed = connection
                .execute(
                    "INSERT INTO capacity_reservation_unbound_retirement_progress (reservation_id, phase, claim_generation, evidence_sha256) SELECT ?1, ?2, ?3, ?4 WHERE NOT EXISTS (SELECT 1 FROM capacity_reservation_unbound_retirement_progress WHERE reservation_id = ?1 AND phase = ?2)",
                    (claim.reservation_id, phase.as_str(), claim.generation.load(Ordering::Acquire), evidence_sha256),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed > 1 {
                return Err(HostError::Journal);
            }
            Ok(())
        })
        .await
    }

    pub(crate) async fn settle_unbound_retirement(
        &self,
        claim: &UnboundRetirementClaim,
    ) -> Result<(), HostError> {
        self.validate_claim_identity(claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_in_transaction(connection, claim).await?;
            settlement::settle(connection, claim).await
        })
        .await
    }

    fn validate_claim_identity(&self, claim: &UnboundRetirementClaim) -> Result<(), HostError> {
        if self.connection_id().as_str() != claim.connection_id
            || claim.reservation_id <= 0
            || claim.generation.load(Ordering::Acquire) <= 0
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

pub(in crate::journal) async fn recover_unbound_pairs_at_startup(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    recovery::normalize_requested(connection).await?;
    abandon_unbound_pairs(connection, None, None).await?;
    connection
        .execute(
            "UPDATE capacity_reservation_unbound_retirement_claims SET claim_generation = claim_generation + 1, lease_until = 1 WHERE lease_until > 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn validate_claim_in_transaction(
    connection: &turso::Connection,
    claim: &UnboundRetirementClaim,
) -> Result<(), HostError> {
    if !owns_cleanup(connection, &claim.connection_id, claim.reservation_id).await? {
        return Err(HostError::Ownership);
    }
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_unbound_retirement_claims AS claim JOIN capacity_reservations AS reservation ON reservation.id = claim.reservation_id WHERE claim.reservation_id = ?1 AND claim.connection_id = ?2 AND claim.operation_id = ?3 AND claim.engine_id = ?4 AND claim.docker_root_digest = ?5 AND claim.claim_generation = ?6 AND claim.lease_until = ?7 AND claim.lease_until > CAST(strftime('%s', 'now') AS INTEGER) AND reservation.state = 'uncertain'",
            (
                claim.reservation_id,
                claim.connection_id.as_str(),
                claim.operation_id,
                claim.engine_id.as_str(),
                claim.docker_root_digest.as_str(),
                claim.generation.load(Ordering::Acquire),
                claim.lease_until.load(Ordering::Acquire),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn unix_seconds() -> Result<i64, HostError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostError::Journal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| HostError::Journal)
}
