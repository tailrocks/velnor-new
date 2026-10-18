//! Connection-scoped journal records and shared host reservations.

mod completion;
mod identity;
mod intents;
mod launch;
mod reservation;
mod runtime;
mod scope;
pub(super) mod storage;

pub(crate) use completion::ConnectionCompletionInboxEntry;
pub(crate) use identity::{AssignmentIdentity, AssignmentKey};
pub(crate) use reservation::{
    CapacityLimits, OperationalCapacityLimits, ReservationRequest, ReservationResult,
    ReservationSnapshot, ReservationState,
};
pub(crate) use runtime::{ConnectionScaleSet, EngineContext, SourceAuthorizationBinding};

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
