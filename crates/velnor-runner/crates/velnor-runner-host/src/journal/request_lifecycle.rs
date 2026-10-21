//! Independent request authorization and charged physical runner lifecycle.

pub(in crate::journal) mod acquire;
pub(in crate::journal) mod authorized_offers;
pub(in crate::journal) mod events;
mod helpers;
pub(in crate::journal) mod physical;
mod set_scope;
pub(in crate::journal) mod source;
#[cfg(test)]
pub(crate) mod tests;

use crate::config::ConnectionId;
#[cfg(test)]
use crate::error::HostError;
#[cfg(test)]
use crate::journal::connection::AssignmentIdentity;

pub(crate) use acquire::AcquiredRecoveryCandidate;
pub(crate) use authorized_offers::AuthorizedOffer;

/// State of one authorized request's `AcquireJobs` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestAcquireState {
    /// The source receipt is durable; `AcquireJobs` has not been claimed.
    Authorized,
    /// The one-shot `AcquireJobs` call may be in flight or interrupted.
    Requested,
    /// The exact request ID was returned by `AcquireJobs`.
    Acquired,
    /// The service returned a definitive no-effect result.
    NoEffect,
    /// The service reported an existing acquisition with unclear ownership.
    AlreadyAcquired,
    /// The service definitively rejected the request.
    Rejected,
    /// The effect or response did not prove an exact single acquisition.
    Uncertain,
}

/// Record of the request selected for one physical runner JIT operation.
///
/// The token is linear: binding or marking uncertainty consumes it.
pub(crate) struct RunnerJitClaim {
    connection_id: ConnectionId,
    scale_set_id: i64,
    intent_id: i64,
    witness_request_id: i64,
    requested_name: String,
}

#[cfg(test)]
impl RunnerJitClaim {
    /// Give the physical capacity intent ID.
    #[must_use]
    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    /// Give the acquired request selected for the JIT audit record.
    #[must_use]
    pub(crate) fn witness_request(&self) -> Result<AssignmentIdentity, HostError> {
        AssignmentIdentity::new(
            &self.connection_id,
            self.scale_set_id,
            crate::journal::connection::AssignmentKey::AssignedRequest {
                request_id: self.witness_request_id,
            },
        )
    }

    /// Give the exact runner name requested from GitHub.
    #[must_use]
    pub(crate) fn runner_name(&self) -> &str {
        &self.requested_name
    }
}
