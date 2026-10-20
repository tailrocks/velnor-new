//! Positive-capacity polling with durable attempt and acquire ownership.

use std::ops::AsyncFnMut;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation};
use crate::listen::Link;
use crate::scale_set::EnsureError;

use super::completion_intake::intake_and_ack_if_only_scoped;
use super::completion_intake::{IntakeDecision, completion_error};
use super::steps::{AcquiredProvisioning, KnownNoAcquire};
use super::{ack_ready, poll_scoped_queue};

/// Result of the one admission callback for an available request.
pub(crate) enum PositiveAdmission {
    /// Keep the Available message on the queue.
    NoAcquire,
    /// The offer has a durable source authorization and can be acknowledged safely.
    Deferred(crate::launch::DeferredAuthorizedOffer),
    /// The journal recorded acquire and reserved its physical capacity.
    Acquired(AcquiredProvisioning),
    /// The journal recorded a definite no-acquire result and its reservation disposition.
    KnownNoAcquire(KnownNoAcquire),
}

/// Queue and journal outcome for one positive-capacity poll.
pub(crate) struct PositivePollOutcome<A = PositiveAdmission> {
    /// Assigned job count from this response, if present.
    pub(crate) assigned_total: Option<i64>,
    /// Message id from the native response. Empty responses have no id.
    pub(crate) message_id: Option<i64>,
    /// Cursor action for an acknowledged response or an unacknowledged queue head.
    pub(crate) cursor_disposition: CursorDisposition,
    /// Native Started events that remain active after durable intake.
    pub(crate) started_request_ids: Vec<i64>,
    /// Whether durable intake found lifecycle work for cleanup.
    pub(crate) wake_cleanup: bool,
    /// Whether this response can support a later session-delete claim.
    pub(crate) session_delete_candidate: bool,
    /// Admission result. This retains an acquired token across ACK uncertainty.
    pub(crate) admission: Option<A>,
    /// Queue acknowledgement status.
    pub(crate) acknowledgement: Acknowledgement,
    /// Durable poll-attempt settlement status.
    pub(crate) settlement: Settlement,
}

/// Cursor action after the controller records the response outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorDisposition {
    /// Read the queue head again because no response id is available.
    Unchanged,
    /// Read the queue head again after an acknowledged response.
    ResetToHead,
    /// Skip this known unacknowledged response until the bounded retry.
    Deferred { message_id: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acknowledgement {
    NotRequired,
    Deferred,
    Acknowledged,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Settlement {
    Settled,
    Uncertain,
}

/// Run one positive poll and one admission callback for an available request.
///
/// This function records the poll attempt before GET. It sends the exact header
/// value from that attempt. It records lifecycle events before any ACK.
///
/// # Errors
///
/// Returns an error if polling, durable intake, admission, or uncertainty
/// recording does not complete.
pub(crate) async fn positive_poll_once<F>(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
    mut admit: F,
) -> Result<PositivePollOutcome, EnsureError>
where
    F: AsyncFnMut(
        &mut Link,
        &Poll,
        Option<String>,
        String,
        String,
        String,
    ) -> Result<PositiveAdmission, EnsureError>,
{
    let attempt = begin_poll(scoped, session_intent_id, set_id, session, allocation).await?;
    let admin_base = link.base().to_owned();
    let (attempt, polled, path, queue, decision) = poll_and_intake(
        link,
        session,
        scoped,
        session_intent_id,
        set_id,
        attempt,
        cursor,
    )
    .await?;
    let (attempt, admission) = invoke_admission(
        link,
        &polled,
        queue.clone(),
        path.clone(),
        session,
        scoped,
        session_intent_id,
        attempt,
        &mut admit,
    )
    .await?;
    finish_positive_poll(
        link,
        session,
        scoped,
        session_intent_id,
        set_id,
        allocation,
        cursor,
        attempt,
        polled,
        path,
        queue,
        admin_base,
        decision,
        admission,
    )
    .await
}

async fn begin_poll(
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    session: &QueueSession,
    allocation: &HeaderAllocation,
) -> Result<crate::journal::HeaderPollAttempt, EnsureError> {
    if allocation.target() == 0
        || !allocation.matches_session(scoped.connection_id(), set_id, &session.session_id)
    {
        return Err(EnsureError::AdmissionClosed);
    }
    let attempt = scoped
        .begin_session_poll(session_intent_id, allocation)
        .await
        .map_err(|_| completion_error())?;
    if attempt.header_value() != allocation.target() || attempt.header_value() == 0 {
        scoped
            .mark_session_poll_uncertain(session_intent_id, attempt)
            .await
            .map_err(|_| completion_error())?;
        return Err(EnsureError::AdmissionClosed);
    }
    Ok(attempt)
}

async fn poll_and_intake(
    link: &mut Link,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    attempt: crate::journal::HeaderPollAttempt,
    cursor: i64,
) -> Result<
    (
        crate::journal::HeaderPollAttempt,
        Poll,
        String,
        Option<String>,
        IntakeDecision,
    ),
    EnsureError,
> {
    let (polled, path, queue) = match poll_scoped_queue(
        link,
        session,
        scoped,
        session_intent_id,
        set_id,
        attempt.header_value(),
        cursor,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            return Err(error);
        }
    };
    let intake = intake_and_ack_if_only_scoped(scoped, set_id, &polled, || async {
        ack_ready(link, session, path.clone(), queue.clone(), &polled)
    })
    .await;
    let decision = match intake {
        Ok(decision) => decision,
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            return Err(error);
        }
    };
    Ok((attempt, polled, path, queue, decision))
}

async fn invoke_admission<F>(
    link: &mut Link,
    polled: &Poll,
    queue: Option<String>,
    path: String,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    attempt: crate::journal::HeaderPollAttempt,
    admit: &mut F,
) -> Result<(crate::journal::HeaderPollAttempt, Option<PositiveAdmission>), EnsureError>
where
    F: AsyncFnMut(
        &mut Link,
        &Poll,
        Option<String>,
        String,
        String,
        String,
    ) -> Result<PositiveAdmission, EnsureError>,
{
    if !contains_available(polled) {
        return Ok((attempt, None));
    }
    let queue_token = session.token().to_owned();
    let admin_token = link.token().to_owned();
    match admit(link, polled, queue, path, queue_token, admin_token).await {
        Ok(admission) => Ok((attempt, Some(admission))),
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            Err(error)
        }
    }
}

mod finish;
pub(crate) use finish::admit_when_ready;
#[cfg(test)]
pub(crate) use finish::finish_positive_poll_with_ack;
#[cfg(test)]
use finish::{
    acknowledgement_after_intake, admit_available, authorized_deferred_matches, cursor_disposition,
    make_outcome, settle_known_response,
};
use finish::{contains_available, finish_positive_poll};

#[cfg(test)]
#[path = "positive_poll_tests.rs"]
mod tests;
