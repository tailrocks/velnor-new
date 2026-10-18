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
    CapacityLimits, ReservationRequest, ReservationResult, ReservationSnapshot, ReservationState,
};
pub(crate) use runtime::{ConnectionScaleSet, EngineContext};

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
