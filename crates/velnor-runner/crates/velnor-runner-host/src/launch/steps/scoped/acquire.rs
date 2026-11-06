// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use velnor_runner_github::{
    AcquireOutcome, RefreshGate, SessionError, Transport, WireError, acquire,
};

use crate::journal::{
    AssignedAcquireResult, AssignmentIdentity, ConnectionJournal, RequestAcquireState,
};
use crate::scale_set::EnsureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcquireDisposition {
    Acquired,
    NoEffect,
    NoEffectSettled,
    NoEffectHeld,
    Rejected,
    RejectedSettled,
    RejectedHeld,
    Uncertain,
}

pub(super) struct ClaimedAcquire {
    identity: AssignmentIdentity,
}

impl ClaimedAcquire {
    pub(super) fn new(identity: AssignmentIdentity) -> Self {
        Self { identity }
    }
}

pub(super) async fn require_authorized(
    scoped: &ConnectionJournal,
    identity: &AssignmentIdentity,
) -> Result<(), EnsureError> {
    let state = scoped
        .request_acquire_state(identity)
        .await
        .map_err(super::super::map_journal)?;
    if state == Some(RequestAcquireState::Authorized) {
        Ok(())
    } else {
        Err(EnsureError::Uncertain)
    }
}

pub(super) async fn run_claimed<T>(
    lane: &mut T,
    queue_token: &str,
    scoped: &ConnectionJournal,
    claim: ClaimedAcquire,
) -> Result<AcquireDisposition, EnsureError>
where
    T: Transport + ?Sized,
{
    let identity = claim.identity;
    let request_id = identity.request_id().ok_or(EnsureError::Uncertain)?;
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::super::map_journal)?;
    if identity.connection_id() != scoped.connection_id()
        || identity.connection_id() != binding.connection_id()
        || identity.scale_set_id() != binding.scale_set_id()
        || scoped
            .request_acquire_state(&identity)
            .await
            .map_err(super::super::map_journal)?
            != Some(RequestAcquireState::Requested)
    {
        return Err(EnsureError::Uncertain);
    }
    let gate = RefreshGate::new();
    let refresh_requested = Cell::new(false);
    let result = acquire(
        lane,
        identity.scale_set_id(),
        &[request_id],
        &[],
        queue_token,
        &gate,
        || {
            refresh_requested.set(true);
            Err::<(), WireError>(WireError::RefreshExhausted)
        },
    );
    let (durable, disposition) = if refresh_requested.get() {
        (
            AssignedAcquireResult::Uncertain,
            AcquireDisposition::Uncertain,
        )
    } else {
        classify(&result, request_id)
    };
    let disposition = match disposition {
        AcquireDisposition::NoEffect => {
            if scoped
                .record_request_acquire_and_settle_unused(&identity, durable)
                .await
                .map_err(super::super::map_journal)?
            {
                AcquireDisposition::NoEffectSettled
            } else {
                AcquireDisposition::NoEffectHeld
            }
        }
        AcquireDisposition::Rejected => {
            if scoped
                .record_request_acquire_and_settle_unused(&identity, durable)
                .await
                .map_err(super::super::map_journal)?
            {
                AcquireDisposition::RejectedSettled
            } else {
                AcquireDisposition::RejectedHeld
            }
        }
        _ => {
            scoped
                .record_request_acquire(&identity, durable)
                .await
                .map_err(super::super::map_journal)?;
            disposition
        }
    };
    match disposition {
        AcquireDisposition::Uncertain if refresh_requested.get() => Err(EnsureError::Uncertain),
        AcquireDisposition::Uncertain => match result {
            Err(error) => Err(crate::listen::map_listen(error)),
            Ok(_) => Err(EnsureError::Uncertain),
        },
        known => Ok(known),
    }
}

pub(super) fn classify(
    result: &Result<AcquireOutcome, SessionError>,
    request_id: i64,
) -> (AssignedAcquireResult, AcquireDisposition) {
    match result {
        Ok(AcquireOutcome::Acquired(ids)) if ids.as_slice() == [request_id] => (
            AssignedAcquireResult::Returned(ids.clone()),
            AcquireDisposition::Acquired,
        ),
        Ok(AcquireOutcome::Acquired(ids)) if ids.is_empty() => (
            AssignedAcquireResult::NoEffect,
            AcquireDisposition::NoEffect,
        ),
        Ok(AcquireOutcome::Acquired(ids)) => (
            AssignedAcquireResult::Returned(ids.clone()),
            AcquireDisposition::Uncertain,
        ),
        Ok(AcquireOutcome::Noop) => (
            AssignedAcquireResult::AlreadyAcquired,
            AcquireDisposition::Uncertain,
        ),
        Err(SessionError::Wire(WireError::RefreshExhausted)) => (
            AssignedAcquireResult::Uncertain,
            AcquireDisposition::Uncertain,
        ),
        Err(error) if error.certainty() == velnor_runner_github::Certainty::Definite => (
            AssignedAcquireResult::Rejected,
            AcquireDisposition::Rejected,
        ),
        Err(_) => (
            AssignedAcquireResult::Uncertain,
            AcquireDisposition::Uncertain,
        ),
    }
}
