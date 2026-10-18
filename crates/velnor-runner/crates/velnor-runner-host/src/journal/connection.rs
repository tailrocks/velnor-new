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
    CapacityLimits, OperationalCapacityLimits, PhysicalRunnerJitState, PhysicalRunnerOwnership,
    PhysicalRunnerReservation, ReservationRequest, ReservationResult, ReservationSnapshot,
    ReservationState, StorageAdmission, StorageAdmissionSample, StorageCacheProjection,
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageMemoryObservation,
    StoragePoolLimit, StoragePoolObservation, StorageProfile, StorageReservationPlan, StorageRole,
    StorageRoleCharge, StorageRoleLimit, StorageSampleIdentity,
};
#[cfg(test)]
pub(crate) use reservation::{fixture_admission, fixture_profile};
pub(crate) use runtime::{ConnectionScaleSet, EngineContext, SourceAuthorizationBinding};
pub(crate) use session::{
    BoundConnectionSession, SessionCreateClaim, SessionCreateRecord, SessionCreateState,
    SessionRefreshClaim, SessionRefreshRecord, SessionRefreshState,
};

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

pub(super) async fn validate_storage_budget_schema(
    connection: &turso::Connection,
) -> Result<(), crate::error::HostError> {
    reservation::validate_storage_budget_schema(connection).await
}
