use std::future::Future;
use std::ops::AsyncFnMut;

use velnor_runner_github::{InnerKind, Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation, HeaderPollAttempt};
use crate::listen::{Link, PollPathResult, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;

use super::super::ack_ready;
use super::completion_intake::{IntakeDecision, completion_error, intake_and_ack_if_only_scoped};
use super::positive_poll::CursorDisposition;
use crate::launch::DeferredAuthorizedOffers;

/// Rules for zero capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZeroHeaderMode {
    /// Save lifecycle events before you acknowledge the response.
    LifecycleOnly,
    /// Approve each Available request before you acknowledge the response.
    DeferAuthorizedOffers,
}

/// Poll one scoped session at a zero header and persist lifecycle events before ACK.
///
/// This path never acquires an Available job or starts a worker. Positive
/// capacity requires the separate fresh-permit admission path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopedPollOutcome {
    /// Assigned job count from this response, if present.
    pub(crate) assigned_total: Option<i64>,
    /// Message id from the native response. Empty responses have no id.
    pub(crate) message_id: Option<i64>,
    /// Cursor action for this response.
    pub(crate) cursor_disposition: CursorDisposition,
    pub(crate) wake_cleanup: bool,
    pub(crate) session_delete_candidate: bool,
    /// Request IDs with a durable native `JobStarted` record.
    pub(crate) started_request_ids: Vec<i64>,
    /// Sanitized counters from this response only.
    pub(crate) observation: crate::launch::steady::PollObservation,
}

/// Borrowed inputs for one exact scoped queue poll.
pub(crate) struct ScopedPollContext<'a> {
    pub(crate) link: &'a mut Link,
    pub(crate) set_id: i64,
    pub(crate) session: &'a mut QueueSession,
    pub(crate) scoped: &'a ConnectionJournal,
    pub(crate) session_intent_id: i64,
    pub(crate) allocation: &'a HeaderAllocation,
    pub(crate) cursor: i64,
}

/// Exact identity and response inputs for one completed scoped poll.
pub(super) struct ScopedPollFinishContext<'a, F> {
    pub(super) scoped: &'a ConnectionJournal,
    pub(super) session_intent_id: i64,
    pub(super) set_id: i64,
    pub(super) session_id: &'a str,
    pub(super) allocation: &'a HeaderAllocation,
    pub(super) polled: &'a Poll,
    pub(super) attempt: HeaderPollAttempt,
    pub(super) handled: F,
    pub(super) cursor: i64,
}

pub(crate) async fn poll_scoped_connection_once(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
) -> Result<ScopedPollOutcome, EnsureError> {
    poll_scoped_connection_once_with_mode(
        ScopedPollContext {
            link,
            set_id,
            session,
            scoped,
            session_intent_id,
            allocation,
            cursor,
        },
        ZeroHeaderMode::LifecycleOnly,
        async |_| Ok(DeferredAuthorizedOffers::empty()),
    )
    .await
}

pub(crate) async fn poll_scoped_connection_once_with_mode<F>(
    context: ScopedPollContext<'_>,
    mode: ZeroHeaderMode,
    mut authorize: F,
) -> Result<ScopedPollOutcome, EnsureError>
where
    F: AsyncFnMut(&Poll) -> Result<DeferredAuthorizedOffers, EnsureError>,
{
    let ScopedPollContext {
        link,
        set_id,
        session,
        scoped,
        session_intent_id,
        allocation,
        cursor,
    } = context;
    if !allocation.matches_session(scoped.connection_id(), set_id, &session.session_id) {
        return Err(EnsureError::AdmissionClosed);
    }
    let attempt = scoped
        .begin_zero_header_poll(session_intent_id, allocation)
        .await
        .map_err(|_| completion_error())?;
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
        Ok(value) => value,
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            return Err(error);
        }
    };
    if attempt.header_value() != 0 {
        scoped
            .mark_session_poll_uncertain(session_intent_id, attempt)
            .await
            .map_err(|_| completion_error())?;
        return Err(EnsureError::AdmissionClosed);
    }
    let handled = async {
        if mode == ZeroHeaderMode::DeferAuthorizedOffers {
            process_deferred_scoped_response(
                scoped,
                set_id,
                &polled,
                async |response| authorize(response).await,
                async || ack_ready(link, session, path.clone(), queue.clone(), &polled),
            )
            .await
        } else {
            let decision = intake_and_ack_if_only_scoped(scoped, set_id, &polled, || async {
                ack_ready(link, session, path.clone(), queue.clone(), &polled)
            })
            .await?;
            let acknowledged = decision.completion_only;
            Ok((decision, acknowledged))
        }
    };
    finish_scoped_poll_response(ScopedPollFinishContext {
        scoped,
        session_intent_id,
        set_id,
        session_id: &session.session_id,
        allocation,
        polled: &polled,
        attempt,
        handled,
        cursor,
    })
    .await
}

pub(super) fn deferred_batch_ackable(polled: &Poll) -> bool {
    let Poll::Batch(batch) = polled else {
        return false;
    };
    batch.message_id >= 0
        && !batch.jobs.is_empty()
        && batch.jobs.len() <= velnor_runner_github::MAX_POLL_MESSAGES
        && batch.jobs.iter().all(|job| {
            matches!(
                job.kind,
                InnerKind::Available
                    | InnerKind::Assigned
                    | InnerKind::Started
                    | InnerKind::Completed
            )
        })
}

pub(super) async fn process_deferred_scoped_response<F, A>(
    scoped: &ConnectionJournal,
    set_id: i64,
    polled: &Poll,
    mut authorize: F,
    mut acknowledge: A,
) -> Result<(IntakeDecision, bool), EnsureError>
where
    F: AsyncFnMut(&Poll) -> Result<DeferredAuthorizedOffers, EnsureError>,
    A: AsyncFnMut() -> Result<(), EnsureError>,
{
    let mut acknowledged = false;
    let decision = intake_and_ack_if_only_scoped(scoped, set_id, polled, || async {
        if acknowledge().await.is_ok() {
            acknowledged = true;
        }
        Ok(())
    })
    .await?;

    if acknowledged || !has_available_offer(polled) || !deferred_batch_ackable(polled) {
        return Ok((decision, acknowledged));
    }

    let Ok(receipts) = authorize(polled).await else {
        return Ok((decision, false));
    };
    if !receipts.matches_current_poll(polled, scoped, set_id).await {
        return Ok((decision, false));
    }
    if acknowledge().await.is_ok() {
        acknowledged = true;
    }
    Ok((decision, acknowledged))
}

fn has_available_offer(polled: &Poll) -> bool {
    matches!(polled, Poll::Batch(batch) if batch.jobs.iter().any(|job| job.kind == InnerKind::Available))
}

pub(super) async fn poll_scoped_queue(
    link: &mut Link,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    total_capacity: u32,
    cursor: i64,
) -> Result<(Poll, String, Option<String>), EnsureError> {
    let (saved, path) = point_at_queue(link, &session.message_queue_url)?;
    let queue = saved.as_ref().map(|_| link.base().to_owned());
    let response = poll_path(link, session, &path, total_capacity, cursor);
    restore_base(link, saved)?;
    match response? {
        PollPathResult::Response(polled) => {
            crate::launch::trace::batch(&polled);
            Ok((polled, path, queue))
        }
        PollPathResult::RefreshRequired => {
            let refreshed = super::super::session::refresh_bound_connection_session(
                link,
                scoped,
                session_intent_id,
                set_id,
                &session.session_id,
            )
            .await?;
            *session = refreshed;

            let (saved, path) = point_at_queue(link, &session.message_queue_url)?;
            let queue = saved.as_ref().map(|_| link.base().to_owned());
            let response = poll_path(link, session, &path, total_capacity, cursor);
            restore_base(link, saved)?;
            match response? {
                PollPathResult::Response(polled) => {
                    crate::launch::trace::batch(&polled);
                    Ok((polled, path, queue))
                }
                // A refreshed bearer that still receives 401 is a terminal
                // refusal for this header attempt. Never replay the same GET.
                PollPathResult::RefreshRequired => Err(EnsureError::Uncertain),
            }
        }
    }
}

pub(super) async fn finish_scoped_poll_response<F>(
    context: ScopedPollFinishContext<'_, F>,
) -> Result<ScopedPollOutcome, EnsureError>
where
    F: Future<Output = Result<(IntakeDecision, bool), EnsureError>>,
{
    let ScopedPollFinishContext {
        scoped,
        session_intent_id,
        set_id,
        session_id,
        allocation,
        polled,
        attempt,
        handled,
        cursor,
    } = context;
    let (mut decision, response_acknowledged) = match handled.await {
        Ok(decision) => decision,
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            return Err(error);
        }
    };
    if cursor > 0 && matches!(polled, Poll::Empty) {
        decision.session_delete_candidate = false;
    }
    if decision.completion_only && !response_acknowledged {
        decision.session_delete_candidate = false;
    }
    let completed = super::super::session::CompletedHeaderPoll::after_response(
        attempt, allocation, scoped, set_id, session_id,
    )?;
    scoped
        .settle_session_poll(session_intent_id, completed)
        .await
        .map_err(|_| completion_error())?;
    Ok(ScopedPollOutcome {
        assigned_total: super::super::steady::assigned_total(polled),
        message_id: super::super::steady::response_message_id(polled),
        cursor_disposition: scoped_cursor_disposition(polled, response_acknowledged),
        wake_cleanup: decision.wake_cleanup,
        session_delete_candidate: decision.session_delete_candidate,
        started_request_ids: decision.started_request_ids,
        observation: super::super::steady::poll_observation(polled),
    })
}

fn scoped_cursor_disposition(polled: &Poll, acknowledged: bool) -> CursorDisposition {
    let message_id = super::super::steady::response_message_id(polled);
    if matches!(polled, Poll::Empty) {
        CursorDisposition::Unchanged
    } else if acknowledged {
        CursorDisposition::ResetToHead
    } else {
        match message_id.filter(|message_id| *message_id > 0) {
            Some(message_id) => CursorDisposition::Deferred { message_id },
            None => CursorDisposition::Unchanged,
        }
    }
}
