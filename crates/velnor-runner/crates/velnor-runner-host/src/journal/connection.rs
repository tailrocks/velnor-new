//! Connection-scoped journal records and shared host reservations.

mod completion;
mod identity;
mod intents;
mod launch;
mod reservation;
mod runtime;
mod scope;
mod session;
pub(super) mod storage;

pub(crate) use completion::ConnectionCompletionInboxEntry;
pub(crate) use identity::{AssignmentIdentity, AssignmentKey};
pub(crate) use reservation::{
    CapacityLimits, OperationalCapacityLimits, ReservationRequest, ReservationResult,
    StorageAdmission, StorageFilesystemKind, StorageInodePolicy, StorageMedium,
    StorageMetadataSettlementClaim, StorageObserver, StorageRole,
};
#[cfg(test)]
pub(crate) use reservation::{
    ListenerDeliveryClaim, MetadataSettlementResult, RequestPhysicalReservationResult,
    ReservationState, StorageAdmissionSample, StorageCacheProjection, StoragePoolLimit,
    StoragePoolObservation, StorageProfile, StorageProvisioningPlan, StorageReservationPlan,
    StorageRoleCharge, StorageRoleLimit, StorageSampleIdentity,
};
#[cfg(test)]
pub(crate) use reservation::{
    fixture_admission, fixture_profile, fixture_provisioning_profile,
    fixture_storage_provisioning_admission,
};
pub(crate) use runtime::{EngineContext, SourceAuthorizationBinding};
#[cfg(test)]
pub(crate) use session::SessionRefreshState;
pub(crate) use session::{SessionCreateState, SessionRefreshClaim};

use crate::config::ConnectionId;
use crate::journal::Journal;

/// Journal operations that belong to one immutable repository connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectionJournal {
    journal: Journal,
    connection_id: ConnectionId,
    identity: String,
    binding_digest: String,
}

pub(in crate::journal) async fn has_unresolved_legacy_work(
    connection: &turso::Connection,
) -> Result<bool, crate::error::HostError> {
    storage::has_ambiguous_legacy_work(connection).await
}

pub(in crate::journal) async fn insert_metadata_settlement_claim(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    seed: &crate::journal::resource_probe::ProbeSeed,
) -> Result<StorageMetadataSettlementClaim, crate::error::HostError> {
    reservation::insert_metadata_settlement_claim(connection, scope, reservation_id, seed).await
}

pub(in crate::journal) async fn record_metadata_probe_cleanup(
    connection: &turso::Connection,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    operation_name: &str,
    container_id: &str,
) -> Result<(), crate::error::HostError> {
    reservation::record_metadata_probe_cleanup(
        connection,
        operation_id,
        engine_id,
        docker_root_digest,
        operation_name,
        container_id,
    )
    .await
}

pub(super) async fn validate_storage_budget_schema(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    reservation::validate_storage_budget_schema(connection).await
}
