//! Durable request authorization and `AcquireJobs` effect.

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
    Rejected,
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

#[cfg(test)]
pub(super) async fn run<T>(
    lane: &mut T,
    queue_token: &str,
    scoped: &ConnectionJournal,
    identity: &AssignmentIdentity,
) -> Result<AcquireDisposition, EnsureError>
where
    T: Transport + ?Sized,
{
    let request_id = identity.request_id().ok_or(EnsureError::Uncertain)?;
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::super::map_journal)?;
    if identity.connection_id() != scoped.connection_id()
        || identity.connection_id() != binding.connection_id()
        || identity.scale_set_id() != binding.scale_set_id()
    {
        return Err(EnsureError::Uncertain);
    }
    require_authorized(scoped, identity).await?;
    if !scoped
        .claim_request_acquire(identity)
        .await
        .map_err(super::super::map_journal)?
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
    scoped
        .record_request_acquire(identity, durable)
        .await
        .map_err(super::super::map_journal)?;
    match disposition {
        AcquireDisposition::Uncertain if refresh_requested.get() => Err(EnsureError::Uncertain),
        AcquireDisposition::Uncertain => match result {
            Err(error) => Err(crate::listen::map_listen(error)),
            Ok(_) => Err(EnsureError::Uncertain),
        },
        known => Ok(known),
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
    scoped
        .record_request_acquire(&identity, durable)
        .await
        .map_err(super::super::map_journal)?;
    match disposition {
        AcquireDisposition::Uncertain if refresh_requested.get() => Err(EnsureError::Uncertain),
        AcquireDisposition::Uncertain => match result {
            Err(error) => Err(crate::listen::map_listen(error)),
            Ok(_) => Err(EnsureError::Uncertain),
        },
        known => Ok(known),
    }
}

#[cfg(test)]
pub(super) async fn release_unstarted(
    scoped: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(), EnsureError> {
    scoped
        .record_remote_cleanup(reservation_id)
        .await
        .map_err(super::super::map_journal)?;
    scoped
        .record_local_cleanup(reservation_id)
        .await
        .map_err(super::super::map_journal)
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

#[cfg(test)]
mod tests {
    use super::{AcquireDisposition, classify};
    use crate::journal::AssignedAcquireResult;
    use velnor_runner_github::{AcquireOutcome, SessionError, WireError};

    #[test]
    fn exact_request_result_allows_one_runner() {
        let (record, disposition) = classify(&Ok(AcquireOutcome::Acquired(vec![41])), 41);
        assert_eq!(record, AssignedAcquireResult::Returned(vec![41]));
        assert_eq!(disposition, AcquireDisposition::Acquired);
    }

    #[test]
    fn empty_or_already_acquired_result_never_allows_a_runner() {
        let empty = classify(&Ok(AcquireOutcome::Acquired(Vec::new())), 41);
        assert_eq!(empty.0, AssignedAcquireResult::NoEffect);
        assert_eq!(empty.1, AcquireDisposition::NoEffect);

        let replay = classify(&Ok(AcquireOutcome::Noop), 41);
        assert_eq!(replay.0, AssignedAcquireResult::AlreadyAcquired);
        assert_eq!(replay.1, AcquireDisposition::Uncertain);
    }

    #[test]
    fn uncertain_or_different_id_result_never_allows_a_runner() {
        let timeout = classify(&Err(SessionError::Uncertain), 41);
        assert_eq!(timeout.0, AssignedAcquireResult::Uncertain);
        assert_eq!(timeout.1, AcquireDisposition::Uncertain);

        let other = classify(&Ok(AcquireOutcome::Acquired(vec![42])), 41);
        assert_eq!(other.0, AssignedAcquireResult::Returned(vec![42]));
        assert_eq!(other.1, AcquireDisposition::Uncertain);
    }

    #[test]
    fn definite_rejection_records_no_effect() {
        let rejected = classify(&Err(SessionError::Wire(WireError::Forbidden)), 41);
        assert_eq!(rejected.0, AssignedAcquireResult::Rejected);
        assert_eq!(rejected.1, AcquireDisposition::Rejected);
    }
}
