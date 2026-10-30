//! Durable pre-provision capacity holds and worker resource evidence.

mod admission;
mod binding;
mod cleanup;
mod containers;
mod objects;
mod publication;

pub(crate) use containers::{BootstrapContainerCreateClaim, BootstrapStartClaim};
pub(crate) use objects::StorageObjectCreateClaim;
pub(crate) use publication::{JobFactsPublicationClaim, ListenerDeliveryClaim};

use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;
use crate::launch::StartPermit;

use super::admission::{ReservationRequest, ReservationResult};
use super::super::ConnectionJournal;
use super::storage_budget::{StorageProfile, StorageProvisioningAdmission};

impl ConnectionJournal {
    /// Read the immutable current policy for trusted sample construction.
    pub(crate) async fn current_storage_profile(&self) -> Result<Option<StorageProfile>, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            super::storage_budget::current_profile(connection).await
        })
        .await
    }

    /// Create one host reservation and full storage hold before Docker changes.
    pub(crate) async fn try_reserve_with_provisioning(
        &self,
        request: ReservationRequest<'_>,
        permit: StartPermit,
        admission: StorageProvisioningAdmission,
    ) -> Result<ReservationResult, HostError> {
        admission::reserve(self, request, permit, admission).await
    }

    /// Persist actual mount and cgroup evidence before JIT or listener delivery.
    pub(crate) async fn bind_storage_resources(
        &self,
        reservation_id: i64,
        binding: crate::launch::bootstrap::VerifiedStorageResourceBinding,
    ) -> Result<(), HostError> {
        binding::bind(self, reservation_id, binding).await
    }

    /// Persist exact container, mount, volume, and cgroup cleanup evidence.
    pub(crate) async fn record_storage_cleanup(
        &self,
        proof: crate::launch::bootstrap::VerifiedStorageCleanup,
    ) -> Result<(), HostError> {
        cleanup::record(self, proof).await
    }
}

pub(in crate::journal) async fn resources_bound(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, HostError> {
    binding::resources_bound(connection, connection_id, intent_id).await
}

pub(in crate::journal) async fn is_v16_reservation(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, HostError> {
    admission::is_v16_reservation(connection, connection_id, intent_id).await
}

pub(in crate::journal) async fn release_ready(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT s.jit_state, s.storage_cleanup_proven, s.bootstrap_start_state, s.resources_bound, s.publication_state, s.delivery_state, (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS o WHERE o.reservation_id = s.reservation_id AND o.create_state != 'unclaimed'), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS b WHERE b.reservation_id = s.reservation_id AND (b.create_state != 'unclaimed' OR b.start_state != 'not_requested')) FROM capacity_reservation_provisioning_state AS s WHERE s.reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(true);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let jit_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let storage_cleaned = row.get::<i64>(1).map_err(|_| HostError::Journal)? == 1;
    let bootstrap_state: String = row.get(2).map_err(|_| HostError::Journal)?;
    let resources_bound = row.get::<i64>(3).map_err(|_| HostError::Journal)? == 1;
    let publication_state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let delivery_state: String = row.get(5).map_err(|_| HostError::Journal)?;
    let object_effects = row.get::<i64>(6).map_err(|_| HostError::Journal)? != 0;
    let container_effects = row.get::<i64>(7).map_err(|_| HostError::Journal)? != 0;
    let no_storage_effect = !object_effects
        && !container_effects
        && !resources_bound
        && bootstrap_state == "not_requested"
        && publication_state == "not_requested"
        && delivery_state == "not_requested";
    let storage_ready = storage_cleaned || no_storage_effect;
    let jit_ready = matches!(jit_state.as_str(), "not_requested" | "cleaned");
    Ok(storage_ready && jit_ready)
}

pub(super) struct HoldContext {
    pub(super) operation_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) profile_generation_id: String,
    pub(super) profile_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) reservation_state: String,
    pub(super) intent_state: String,
    pub(super) cleanup_proven: bool,
}

pub(super) async fn hold_context(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<Option<HoldContext>, HostError> {
    let mut rows = connection
        .query(
            "SELECT p.operation_id, i.scale_set_id, p.engine_id, p.docker_root_digest, p.profile_generation_id, p.profile_digest, p.sample_generation_id, r.state, i.state, i.cleanup_proven FROM capacity_reservation_provisioning AS p JOIN capacity_reservations AS r ON r.id = p.reservation_id AND r.connection_id = ?2 JOIN connection_intents AS i ON i.id = p.operation_id AND i.connection_id = r.connection_id WHERE p.reservation_id = ?1 AND i.kind = 'launch' AND i.scale_set_id IS NOT NULL",
            (reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(HoldContext {
        operation_id: row.get(0).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(1).map_err(|_| HostError::Journal)?,
        engine_id: row.get(2).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(3).map_err(|_| HostError::Journal)?,
        profile_generation_id: row.get(4).map_err(|_| HostError::Journal)?,
        profile_digest: row.get(5).map_err(|_| HostError::Journal)?,
        sample_generation_id: row.get(6).map_err(|_| HostError::Journal)?,
        reservation_state: row.get(7).map_err(|_| HostError::Journal)?,
        intent_state: row.get(8).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get::<i64>(9).map_err(|_| HostError::Journal)? != 0,
    }))
}

pub(super) fn hold_is_claimable(context: &HoldContext) -> bool {
    context.reservation_state == "reserved"
        && context.intent_state == "pending"
        && !context.cleanup_proven
}

pub(super) async fn mark_hold_uncertain(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    operation_id: i64,
) -> Result<(), HostError> {
    let reservation_changed = connection
        .execute(
            "UPDATE capacity_reservations SET state = 'uncertain' WHERE id = ?1 AND connection_id = ?2 AND intent_id = ?3 AND state = 'reserved' AND remote_cleanup_proven = 0 AND local_cleanup_proven = 0",
            (reservation_id, connection_id, operation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let intent_changed = connection
        .execute(
            "UPDATE connection_intents SET state = 'uncertain' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0",
            (operation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if reservation_changed == 1 && intent_changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
