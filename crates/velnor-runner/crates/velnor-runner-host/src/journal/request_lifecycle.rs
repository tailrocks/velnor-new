// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(in crate::journal) mod acquire;
pub(in crate::journal) mod authorized_offers;
pub(in crate::journal) mod events;
mod helpers;
pub(in crate::journal) mod physical;
mod set_scope;
pub(in crate::journal) mod source;

use crate::config::ConnectionId;

pub(crate) use acquire::AcquiredRecoveryCandidate;
pub(crate) use authorized_offers::AuthorizedOffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestAcquireState {
    Authorized,

    Requested,

    Acquired,

    NoEffect,

    AlreadyAcquired,

    Rejected,

    Uncertain,
}

pub(crate) struct RunnerJitClaim {
    connection_id: ConnectionId,
    scale_set_id: i64,
    intent_id: i64,
    witness_request_id: i64,
    requested_name: String,
}
