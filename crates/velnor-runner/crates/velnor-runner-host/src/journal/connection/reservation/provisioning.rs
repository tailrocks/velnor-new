//! Journal records for capacity reservations and worker resource evidence.

mod admission;
mod binding;
mod cleanup;
pub(crate) use cleanup::StorageCleanupIdentity;
pub(crate) use cleanup::StorageCleanupObserverWitness;
#[cfg(test)]
pub(crate) use cleanup::{StorageCleanupCgroup, StorageCleanupMount};
mod containers;
pub(crate) use containers::{BootstrapContainerCreateClaim, BootstrapStartClaim};
mod initialization;
mod metadata_settlement;
mod network;
mod objects;
mod pre_jit_cleanup;
mod publication;
mod request_link;
mod storage_plan;

#[cfg(test)]
pub(crate) use initialization::{CreateCommitStage, arm_create_commit_gate};
pub(crate) use initialization::{CreateInterruptionOutcome, StorageInitializationClaim};
pub(crate) use initialization::{
    VerifiedMetadataInitialization, VerifiedMetadataInitializationHandoff,
};
pub(crate) use metadata_settlement::{MetadataSettlementResult, StorageMetadataSettlementClaim};
#[cfg(test)]
pub(crate) use network::StorageNetworkRecoveryIdentity;
pub(crate) use network::{
    RecordedStorageNetwork, StorageNetworkCreateClaim, StorageNetworkRemovalClaim,
};
pub(crate) use objects::StorageObjectCreateClaim;
pub(crate) use pre_jit_cleanup::PreJitCapacityProbeClaim;
pub(crate) use pre_jit_cleanup::PreJitCleanupClaim;
pub(crate) use pre_jit_cleanup::PreJitCleanupSettlementResult;
pub(crate) use pre_jit_cleanup::{
    PreJitBootstrapContainer, PreJitCleanupInventory, PreJitCreateSpec, PreJitInitializerIdentity,
    PreJitNetworkIdentity, PreJitStorageObject,
};
pub(crate) use publication::JobFactsPublicationClaim;
pub(crate) use publication::ListenerDeliveryClaim;
pub(crate) use request_link::{
    PhysicalSuccessorReservationResult, RequestPhysicalReservationResult,
};
pub(crate) use storage_plan::{StorageProvisioningPlan, StorageProvisioningRole};

pub(in crate::journal) async fn insert_metadata_settlement_claim(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    seed: &crate::journal::resource_probe::ProbeSeed,
) -> Result<StorageMetadataSettlementClaim, HostError> {
    metadata_settlement::insert_claim(connection, scope, reservation_id, seed).await
}

pub(in crate::journal) async fn record_metadata_probe_cleanup(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    operation_name: &str,
    container_id: &str,
) -> Result<(), HostError> {
    metadata_settlement::record_probe_cleanup(
        connection,
        operation_id,
        engine_id,
        docker_root_digest,
        operation_name,
        container_id,
    )
    .await
}

use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;
use crate::launch::StartPermit;

use super::super::ConnectionJournal;
use super::admission::{ReservationRequest, ReservationResult};
use super::storage_budget::{StorageProfile, StorageProvisioningAdmission};

impl ConnectionJournal {
    /// Read the immutable current policy for trusted sample construction.
    pub(crate) async fn current_storage_profile(
        &self,
    ) -> Result<Option<StorageProfile>, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            super::storage_budget::current_profile(connection).await
        })
        .await
    }

    /// Read the exact role plan for a held reservation after its request is acquired.
    pub(crate) async fn storage_provisioning_plan(
        &self,
        reservation_id: i64,
    ) -> Result<Option<StorageProvisioningPlan>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if context.reservation_state != "reserved"
                || context.intent_state != "pending"
                || context.cleanup_proven
            {
                return Ok(None);
            }
            let mut rows = connection
                .query(
                    "SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id WHERE current.reservation_id = ?1 AND attempt.connection_id = ?2 AND auth.acquire_state = 'acquired'",
                    (reservation_id, connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
                let mut links = connection
                    .query(
                        "SELECT 1 FROM physical_attempt_current WHERE reservation_id = ?1",
                        (reservation_id, connection_id.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                if links.next().await.map_err(|_| HostError::Journal)?.is_some() {
                    return Ok(None);
                }
            } else if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            storage_plan::load(connection, reservation_id, &context)
                .await
                .map(Some)
        })
        .await
    }

    /// Record one host capacity reservation and the full storage role charges before Docker changes.
    pub(crate) async fn try_reserve_with_provisioning(
        &self,
        request: ReservationRequest<'_>,
        permit: StartPermit,
        admission: StorageProvisioningAdmission,
    ) -> Result<ReservationResult, HostError> {
        admission::reserve(self, request, permit, admission).await
    }

    /// Record actual mount and cgroup evidence before JIT or listener delivery.
    pub(crate) async fn bind_storage_resources(
        &self,
        reservation_id: i64,
        binding: &crate::launch::bootstrap::VerifiedStorageResourceBinding,
    ) -> Result<(), HostError> {
        binding::bind(self, reservation_id, binding).await
    }

    /// Read the saved facts for one cleanup observer.
    pub(crate) async fn storage_cleanup_witness(
        &self,
        reservation_id: i64,
    ) -> Result<Option<cleanup::StorageCleanupObserverWitness>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            cleanup::read_observer_witness(connection, &connection_id, reservation_id).await
        })
        .await
    }

    /// Record exact container, mount, volume, and cgroup cleanup evidence.
    pub(crate) async fn record_storage_cleanup(
        &self,
        proof: crate::launch::bootstrap::VerifiedStorageCleanup,
    ) -> Result<(), HostError> {
        cleanup::record(self, proof).await
    }

    /// Prepare one post-cleanup probe. Record its capacity reservation connection before the first Docker effect.
    pub(crate) async fn prepare_storage_metadata_settlement_probe(
        &self,
        reservation_id: i64,
        seed: crate::journal::resource_probe::ProbeSeed,
    ) -> Result<StorageMetadataSettlementClaim, HostError> {
        self.journal_ref()
            .prepare_probe_for_storage_settlement(self, reservation_id, seed)
            .await
    }

    /// Record the fresh metadata sample for this capacity reservation. Release capacity only when all cleanup proofs permit release.
    pub(crate) async fn record_storage_metadata_settlement(
        &self,
        claim: &StorageMetadataSettlementClaim,
        sample: super::storage_budget::StorageAdmissionSample,
    ) -> Result<MetadataSettlementResult, HostError> {
        metadata_settlement::record(self, claim, sample).await
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
            "SELECT s.jit_state, s.storage_cleanup_proven, s.bootstrap_start_state, s.resources_bound, s.publication_state, s.delivery_state, (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS o WHERE o.reservation_id = s.reservation_id AND o.create_state != 'unclaimed'), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS b WHERE b.reservation_id = s.reservation_id AND (b.create_state != 'unclaimed' OR b.start_state != 'not_requested')), (SELECT COUNT(*) FROM capacity_reservation_metadata_settlements AS m WHERE m.reservation_id = s.reservation_id AND m.generation = (SELECT MAX(c.generation) FROM capacity_reservation_metadata_settlement_claims AS c WHERE c.reservation_id = s.reservation_id) AND m.outcome = 'settled'), (SELECT COUNT(*) FROM capacity_reservation_storage_initialization AS init WHERE init.reservation_id = s.reservation_id AND init.remove_state != 'removed' AND init.abort_state != 'absent') FROM capacity_reservation_provisioning_state AS s WHERE s.reservation_id = ?1",
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
    let metadata_settled = row.get::<i64>(8).map_err(|_| HostError::Journal)? == 1;
    let initializer_unsettled = row.get::<i64>(9).map_err(|_| HostError::Journal)? != 0;
    let no_storage_effect = !object_effects
        && !container_effects
        && !initializer_unsettled
        && !resources_bound
        && bootstrap_state == "not_requested"
        && publication_state == "not_requested"
        && delivery_state == "not_requested";
    let storage_ready =
        !initializer_unsettled && (no_storage_effect || (storage_cleaned && metadata_settled));
    let jit_ready = matches!(jit_state.as_str(), "not_requested" | "cleaned");
    let network_ready = network::release_ready(connection, reservation_id).await?;
    Ok(storage_ready && jit_ready && network_ready)
}

pub(super) struct HoldContext {
    pub(super) operation_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) profile_generation_id: String,
    pub(super) profile_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) topology_version: i64,
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
            "SELECT p.operation_id, i.scale_set_id, p.engine_id, p.docker_root_digest, p.profile_generation_id, p.profile_digest, p.sample_generation_id, t.topology_version, r.state, i.state, i.cleanup_proven FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_topology AS t ON t.reservation_id = p.reservation_id JOIN capacity_reservations AS r ON r.id = p.reservation_id AND r.connection_id = ?2 JOIN connection_intents AS i ON i.id = p.operation_id AND i.connection_id = r.connection_id WHERE p.reservation_id = ?1 AND i.kind = 'launch' AND i.scale_set_id IS NOT NULL",
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
        topology_version: row.get(7).map_err(|_| HostError::Journal)?,
        reservation_state: row.get(8).map_err(|_| HostError::Journal)?,
        intent_state: row.get(9).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get::<i64>(10).map_err(|_| HostError::Journal)? != 0,
    }))
}

pub(super) fn hold_is_claimable(context: &HoldContext) -> bool {
    context.reservation_state == "reserved"
        && context.intent_state == "pending"
        && !context.cleanup_proven
        && context.topology_version == 2
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
