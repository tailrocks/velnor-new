use std::future::Future;
use std::ops::AsyncFnMut;

use velnor_runner_github::{InnerKind, Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation};
use crate::listen::Link;
use crate::scale_set::EnsureError;

use super::super::ack_ready;
use super::super::completion_intake::IntakeDecision;
use super::{
    Acknowledgement, CursorDisposition, PositiveAdmission, PositivePollOutcome, Settlement,
};

pub(super) async fn finish_positive_poll(
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
    finish_positive_poll_with_ack(
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
        async |link, session, admin_base, path, queue, polled| {
            acknowledge_response(link, session, admin_base, path, queue, polled).await
        },
    )
    .await
}

pub(crate) async fn finish_positive_poll_with_ack<F>(
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
    mut acknowledge: F,
) -> Result<PositivePollOutcome, EnsureError>
where
    F: AsyncFnMut(&mut Link, &QueueSession, &str, &str, &Option<String>, &Poll) -> Acknowledgement,
{
    let mut decision = decision;
    if cursor > 0 && matches!(polled, Poll::Empty) {
        decision.session_delete_candidate = false;
    }
    let mut acknowledgement = acknowledgement_after_intake(&decision, &polled);
    if !acknowledge_admission(
        link,
        session,
        scoped,
        set_id,
        &polled,
        &path,
        &queue,
        &admin_base,
        &admission,
        &mut acknowledge,
        &mut acknowledgement,
    )
    .await
    {
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

async fn acknowledge_admission<F>(
    link: &mut Link,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    set_id: i64,
    polled: &Poll,
    path: &str,
    queue: &Option<String>,
    admin_base: &str,
    admission: &Option<PositiveAdmission>,
    acknowledge: &mut F,
    acknowledgement: &mut Acknowledgement,
) -> bool
where
    F: AsyncFnMut(&mut Link, &QueueSession, &str, &str, &Option<String>, &Poll) -> Acknowledgement,
{
    if let Some(PositiveAdmission::KnownNoAcquire(receipt)) = admission {
        if !receipt.matches_poll(polled, scoped, set_id) {
            return false;
        }
        if receipt.state() == crate::journal::RequestAcquireState::NoEffect
            && receipt.reservation_released()
        {
            *acknowledgement = acknowledge(link, session, admin_base, path, queue, polled).await;
        }
    }
    if let Some(PositiveAdmission::Deferred(receipt)) = admission {
        if !authorized_deferred_matches(receipt, polled, scoped, set_id).await {
            return false;
        }
        *acknowledgement = acknowledge(link, session, admin_base, path, queue, polled).await;
    }
    if acquired(admission) {
        if !acquired_matches(admission, polled, scoped, set_id) {
            return false;
        }
        *acknowledgement = acknowledge(link, session, admin_base, path, queue, polled).await;
    }
    true
}

pub(super) async fn settle_known_response(
    attempt: crate::journal::HeaderPollAttempt,
    allocation: &HeaderAllocation,
    scoped: &ConnectionJournal,
    set_id: i64,
    session_id: &str,
    session_intent_id: i64,
) -> Settlement {
    let Ok(completed) = super::super::super::session::CompletedHeaderPoll::after_response(
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

pub(super) fn contains_available(polled: &Poll) -> bool {
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

pub(super) async fn admit_available<F, Fut>(
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

pub(super) async fn authorized_deferred_matches(
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
    let Ok(Some((_, request_id))) = crate::launch::steps::assignment(polled) else {
        return false;
    };
    let identity = token.request();
    identity.connection_id() == scoped.connection_id()
        && identity.scale_set_id() == set_id
        && identity.request_id() == Some(request_id)
}

pub(super) fn acknowledgement_after_intake(
    decision: &IntakeDecision,
    polled: &Poll,
) -> Acknowledgement {
    if decision.completion_only {
        Acknowledgement::Acknowledged
    } else if matches!(polled, Poll::Empty) {
        Acknowledgement::NotRequired
    } else {
        Acknowledgement::Deferred
    }
}

pub(super) fn make_outcome<A>(
    decision: IntakeDecision,
    admission: Option<A>,
    acknowledgement: Acknowledgement,
    settlement: Settlement,
    polled: &Poll,
) -> PositivePollOutcome<A> {
    let message_id = crate::launch::steady::response_message_id(polled);
    let cursor_disposition = cursor_disposition(message_id, acknowledgement, polled);
    PositivePollOutcome {
        assigned_total: crate::launch::steady::assigned_total(polled),
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

pub(super) fn cursor_disposition(
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
