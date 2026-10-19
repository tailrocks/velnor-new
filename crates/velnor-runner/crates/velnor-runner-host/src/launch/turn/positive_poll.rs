//! Positive-capacity polling with durable attempt and acquire ownership.

use std::future::Future;
use std::ops::AsyncFnMut;

use velnor_runner_github::{InnerKind, Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation};
use crate::listen::Link;
use crate::scale_set::EnsureError;

use super::completion_intake::intake_and_ack_if_only_scoped;
use super::completion_intake::{IntakeDecision, completion_error};
use super::steps::AcquiredProvisioning;
use super::{ack_ready, poll_scoped_queue};

/// Result of the one admission callback for an available request.
pub(crate) enum PositiveAdmission {
    /// Keep the Available message on the queue.
    NoAcquire,
    /// The offer has a durable source authorization and can be acknowledged safely.
    Deferred(crate::launch::DeferredAuthorizedOffer),
    /// The journal recorded acquire and reserved its physical capacity.
    Acquired(AcquiredProvisioning),
}

/// Queue and journal outcome for one positive-capacity poll.
pub(crate) struct PositivePollOutcome<A = PositiveAdmission> {
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

async fn finish_positive_poll(
    link: &mut Link,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
    attempt: crate::journal::HeaderPollAttempt,
    polled: Poll,
    path: String,
    queue: Option<String>,
    admin_base: String,
    decision: IntakeDecision,
    admission: Option<PositiveAdmission>,
) -> Result<PositivePollOutcome, EnsureError> {
    let mut decision = decision;
    if cursor > 0 && matches!(polled, Poll::Empty) {
        decision.session_delete_candidate = false;
    }
    let mut acknowledgement = acknowledgement_after_intake(&decision, &polled);
    if let Some(PositiveAdmission::Deferred(receipt)) = &admission {
        if !authorized_deferred_matches(receipt, &polled, scoped, set_id).await {
            let _uncertainty_recorded = scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await;
            return Ok(make_outcome(
                decision,
                admission,
                Acknowledgement::Deferred,
                Settlement::Uncertain,
                &polled,
            ));
        }
        acknowledgement =
            acknowledge_response(link, session, &admin_base, &path, &queue, &polled).await;
    }
    if acquired(&admission) {
        if !acquired_matches(&admission, &polled, scoped, set_id) {
            let _uncertainty_recorded = scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await;
            return Ok(make_outcome(
                decision,
                admission,
                Acknowledgement::Deferred,
                Settlement::Uncertain,
                &polled,
            ));
        }
        acknowledgement =
            acknowledge_response(link, session, &admin_base, &path, &queue, &polled).await;
    }

    let settlement = settle_known_response(
        attempt,
        allocation,
        scoped,
        set_id,
        &session.session_id,
        session_intent_id,
    )
    .await;
    Ok(make_outcome(
        decision,
        admission,
        acknowledgement,
        settlement,
        &polled,
    ))
}

async fn settle_known_response(
    attempt: crate::journal::HeaderPollAttempt,
    allocation: &HeaderAllocation,
    scoped: &ConnectionJournal,
    set_id: i64,
    session_id: &str,
    session_intent_id: i64,
) -> Settlement {
    let Ok(completed) = super::super::session::CompletedHeaderPoll::after_response(
        attempt, allocation, scoped, set_id, session_id,
    ) else {
        return Settlement::Uncertain;
    };
    if scoped
        .settle_session_poll(session_intent_id, completed)
        .await
        .is_ok()
    {
        Settlement::Settled
    } else {
        Settlement::Uncertain
    }
}

fn contains_available(polled: &Poll) -> bool {
    matches!(polled, Poll::Batch(batch) if batch.jobs.iter().any(|job| job.kind == InnerKind::Available))
}

async fn acknowledge_response(
    link: &mut Link,
    session: &QueueSession,
    admin_base: &str,
    path: &str,
    queue: &Option<String>,
    polled: &Poll,
) -> Acknowledgement {
    if link.set_base(admin_base).is_err() {
        return Acknowledgement::Deferred;
    }
    if ack_ready(link, session, path.to_owned(), queue.clone(), polled).is_err() {
        Acknowledgement::Uncertain
    } else {
        Acknowledgement::Acknowledged
    }
}

async fn admit_available<F, Fut>(
    polled: &Poll,
    admit: F,
) -> Result<Option<PositiveAdmission>, EnsureError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<PositiveAdmission, EnsureError>>,
{
    if contains_available(polled) {
        admit().await.map(Some)
    } else {
        Ok(None)
    }
}

/// Keep a native offer on the queue until the verified admission gate is open.
pub(crate) async fn admit_when_ready<F, Fut>(
    admission_ready: bool,
    acquire: F,
) -> Result<PositiveAdmission, EnsureError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<PositiveAdmission, EnsureError>>,
{
    if admission_ready {
        acquire().await
    } else {
        Ok(PositiveAdmission::NoAcquire)
    }
}

fn acquired(admission: &Option<PositiveAdmission>) -> bool {
    matches!(admission, Some(PositiveAdmission::Acquired(_)))
}

async fn authorized_deferred_matches(
    receipt: &crate::launch::DeferredAuthorizedOffer,
    polled: &Poll,
    scoped: &ConnectionJournal,
    set_id: i64,
) -> bool {
    receipt
        .matches_current_envelope(polled, scoped, set_id)
        .await
}

fn acquired_matches(
    admission: &Option<PositiveAdmission>,
    polled: &Poll,
    scoped: &ConnectionJournal,
    set_id: i64,
) -> bool {
    let Some(PositiveAdmission::Acquired(token)) = admission else {
        return false;
    };
    let Ok(Some((_, request_id))) = super::steps::assignment(polled) else {
        return false;
    };
    let identity = token.request();
    identity.connection_id() == scoped.connection_id()
        && identity.scale_set_id() == set_id
        && identity.request_id() == Some(request_id)
}

fn acknowledgement_after_intake(decision: &IntakeDecision, polled: &Poll) -> Acknowledgement {
    if decision.completion_only {
        Acknowledgement::Acknowledged
    } else if matches!(polled, Poll::Empty) {
        Acknowledgement::NotRequired
    } else {
        Acknowledgement::Deferred
    }
}

fn make_outcome<A>(
    decision: IntakeDecision,
    admission: Option<A>,
    acknowledgement: Acknowledgement,
    settlement: Settlement,
    polled: &Poll,
) -> PositivePollOutcome<A> {
    let message_id = super::super::steady::response_message_id(polled);
    let cursor_disposition = cursor_disposition(message_id, acknowledgement, polled);
    PositivePollOutcome {
        message_id,
        cursor_disposition,
        started_request_ids: decision.started_request_ids,
        wake_cleanup: decision.wake_cleanup,
        session_delete_candidate: decision.session_delete_candidate,
        admission,
        acknowledgement,
        settlement,
    }
}

fn cursor_disposition(
    message_id: Option<i64>,
    acknowledgement: Acknowledgement,
    polled: &Poll,
) -> CursorDisposition {
    if matches!(polled, Poll::Empty) {
        CursorDisposition::Unchanged
    } else if matches!(
        acknowledgement,
        Acknowledgement::Deferred | Acknowledgement::Uncertain
    ) {
        match message_id.filter(|message_id| *message_id > 0) {
            Some(message_id) => CursorDisposition::Deferred { message_id },
            None => CursorDisposition::Unchanged,
        }
    } else {
        CursorDisposition::ResetToHead
    }
}

#[cfg(test)]
#[path = "positive_poll_tests.rs"]
mod tests;
