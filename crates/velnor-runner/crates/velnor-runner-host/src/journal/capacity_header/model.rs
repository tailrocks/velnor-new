// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::config::ConnectionId;

pub(super) const MAX_HEADER_CAPACITY: u32 = i32::MAX as u32;
pub(super) const MAX_SESSION_ID_BYTES: usize = 256;

#[expect(
    clippy::struct_field_names,
    reason = "these fields form one immutable connection and session identity"
)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeaderSessionIdentity {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct SessionId(pub(super) String);

impl SessionId {
    pub(super) fn parse(value: &str) -> Result<Self, HeaderLedgerError> {
        if value.is_empty()
            || value.len() > MAX_SESSION_ID_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(HeaderLedgerError::InvalidSessionId);
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct HeaderGeneration(pub(super) u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderAllocation {
    pub(super) identity: HeaderSessionIdentity,
    pub(super) generation: HeaderGeneration,
    pub(super) target: u32,
}

impl HeaderAllocation {
    #[must_use]
    pub(crate) fn connection_id(&self) -> &ConnectionId {
        &self.identity.connection_id
    }

    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.identity.scale_set_id
    }

    #[must_use]
    pub(crate) fn session_id(&self) -> &str {
        &self.identity.session_id.0
    }

    #[must_use]
    pub(crate) const fn target(&self) -> u32 {
        self.target
    }

    #[must_use]
    pub(crate) fn matches_session(
        &self,
        connection_id: &ConnectionId,
        scale_set_id: i64,
        session_id: &str,
    ) -> bool {
        self.identity.connection_id == *connection_id
            && self.identity.scale_set_id == scale_set_id
            && self.identity.session_id.0 == session_id
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HeaderPollAttempt {
    pub(super) identity: HeaderSessionIdentity,
    pub(super) generation: HeaderGeneration,
    pub(super) attempt: u64,
    pub(super) header_value: u32,
    pub(super) purpose: PollPurpose,
}

impl HeaderPollAttempt {
    #[must_use]
    pub(crate) fn matches_session(
        &self,
        connection_id: &ConnectionId,
        scale_set_id: i64,
        session_id: &str,
    ) -> bool {
        self.identity.connection_id == *connection_id
            && self.identity.scale_set_id == scale_set_id
            && self.identity.session_id.0 == session_id
    }

    #[must_use]
    pub(crate) const fn header_value(&self) -> u32 {
        self.header_value
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HeaderDeleteAttempt {
    pub(super) identity: HeaderSessionIdentity,
    pub(super) generation: HeaderGeneration,
    pub(super) sequence: u64,
}

impl HeaderDeleteAttempt {
    #[must_use]
    pub(crate) fn matches_session(
        &self,
        connection_id: &ConnectionId,
        scale_set_id: i64,
        session_id: &str,
    ) -> bool {
        self.identity.connection_id == *connection_id
            && self.identity.scale_set_id == scale_set_id
            && self.identity.session_id.0 == session_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollOutcome {
    Response,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum PollPurpose {
    Advertise,
    ZeroHeader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InFlightPoll {
    pub(super) sequence: u64,
    pub(super) header_value: u32,
    pub(super) purpose: PollPurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InFlightDelete {
    pub(super) sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum GenerationState {
    Pending,
    Observed,
    Uncertain,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeaderRecord {
    pub(super) identity: HeaderSessionIdentity,
    pub(super) generation: HeaderGeneration,
    pub(super) target: u32,
    pub(super) state: GenerationState,
    pub(super) attempt_sequence: u64,
    pub(super) in_flight_poll: Option<InFlightPoll>,
    pub(super) zero_header_settled: bool,
    #[serde(default)]
    pub(super) delete_sequence: u64,
    #[serde(default)]
    pub(super) in_flight_delete: Option<InFlightDelete>,
    #[serde(default)]
    pub(super) delete_uncertain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) pressure_capacity: Option<u32>,
}

impl HeaderRecord {
    pub(super) fn allocation(&self) -> HeaderAllocation {
        HeaderAllocation {
            identity: self.identity.clone(),
            generation: self.generation,
            target: self.target,
        }
    }

    pub(super) const fn is_charged(&self) -> bool {
        !matches!(self.state, GenerationState::Released)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeaderLedgerSnapshot {
    pub(super) max_jobs: u32,
    pub(super) next_generation: u64,
    pub(super) records: Vec<HeaderRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum HeaderLedgerError {
    #[error("host max_jobs must be positive")]
    ZeroHostCapacity,
    #[error("Scale Set ID must be positive")]
    InvalidScaleSetId,
    #[error("session ID is empty, too long, or contains control characters")]
    InvalidSessionId,
    #[error("header is outside the GitHub listener range")]
    HeaderOutOfRange,
    #[error("host-wide header capacity would exceed max_jobs")]
    OverBudget,
    #[error("aggregate header capacity overflowed")]
    CapacityOverflow,
    #[error("another session for this connection and Scale Set remains charged")]
    SessionStillCharged,
    #[error("header target changed for an immutable session generation")]
    TargetChanged,
    #[error("this session generation has already been retired")]
    SessionRetired,
    #[error("allocation does not identify the current generation")]
    StaleAllocation,
    #[error("allocation does not exist")]
    UnknownAllocation,
    #[error("a poll is already in flight for this session generation")]
    PollAlreadyInFlight,
    #[error("the session's zero-header poll has already settled")]
    ZeroHeaderSettled,
    #[error("the session's zero-header poll has not settled")]
    ZeroHeaderNotSettled,
    #[error("a session deletion request is already in flight")]
    DeleteAlreadyInFlight,
    #[error("an earlier session deletion has an unresolved remote outcome")]
    DeleteOutcomeUncertain,
    #[error("session deletion is not ready for this allocation")]
    DeleteNotReady,
    #[error("poll attempt counter overflowed")]
    AttemptOverflow,
    #[error("poll result is not for the current in-flight attempt")]
    StaleAttempt,
    #[error("a poll is still in flight; release is forbidden")]
    PollStillInFlight,
    #[error("an earlier poll has an unresolved remote outcome")]
    UnsettledPoll,
    #[error("header generation counter overflowed")]
    GenerationOverflow,
    #[error("persisted header ledger is malformed or violates its invariants")]
    InvalidSnapshot,
}
