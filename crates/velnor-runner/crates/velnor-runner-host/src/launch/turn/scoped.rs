use std::future::Future;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation, HeaderPollAttempt};
use crate::listen::{Link, PollPathResult, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;

use super::super::ack_ready;
use super::completion_intake::{IntakeDecision, completion_error, intake_and_ack_if_only_scoped};
use super::positive_poll::CursorDisposition;

/// Poll one scoped session at a zero header and persist lifecycle events before ACK.
///
/// This path never acquires an Available job or starts a worker. Positive
/// capacity requires the separate fresh-permit admission path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopedPollOutcome {
    /// Message id from the native response. Empty responses have no id.
    pub(crate) message_id: Option<i64>,
    /// Cursor action for this response.
    pub(crate) cursor_disposition: CursorDisposition,
    pub(crate) wake_cleanup: bool,
    pub(crate) session_delete_candidate: bool,
    /// Request IDs with a durable native `JobStarted` record.
    pub(crate) started_request_ids: Vec<i64>,
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
        0,
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
    let intake = intake_and_ack_if_only_scoped(scoped, set_id, &polled, || async {
        ack_ready(link, session, path, queue, &polled)
    });
    let handled = intake;
    finish_scoped_poll_response(
        scoped,
        session_intent_id,
        set_id,
        &session.session_id,
        allocation,
        &polled,
        attempt,
        handled,
        cursor,
    )
    .await
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
        PollPathResult::Response(polled) => Ok((polled, path, queue)),
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
                PollPathResult::Response(polled) => Ok((polled, path, queue)),
                // A refreshed bearer that still receives 401 is a terminal
                // refusal for this header attempt. Never replay the same GET.
                PollPathResult::RefreshRequired => Err(EnsureError::Uncertain),
            }
        }
    }
}

pub(super) async fn finish_scoped_poll_response<F>(
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    session_id: &str,
    allocation: &HeaderAllocation,
    polled: &Poll,
    attempt: HeaderPollAttempt,
    handled: F,
    cursor: i64,
) -> Result<ScopedPollOutcome, EnsureError>
where
    F: Future<Output = Result<IntakeDecision, EnsureError>>,
{
    let mut decision = match handled.await {
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
    let completed = super::super::session::CompletedHeaderPoll::after_response(
        attempt, allocation, scoped, set_id, session_id,
    )?;
    scoped
        .settle_session_poll(session_intent_id, completed)
        .await
        .map_err(|_| completion_error())?;
    Ok(ScopedPollOutcome {
        message_id: super::super::steady::response_message_id(polled),
        cursor_disposition: scoped_cursor_disposition(polled, &decision),
        wake_cleanup: decision.wake_cleanup,
        session_delete_candidate: decision.session_delete_candidate,
        started_request_ids: decision.started_request_ids,
    })
}

fn scoped_cursor_disposition(polled: &Poll, decision: &IntakeDecision) -> CursorDisposition {
    let message_id = super::super::steady::response_message_id(polled);
    if matches!(polled, Poll::Empty) {
        CursorDisposition::Unchanged
    } else if decision.completion_only {
        CursorDisposition::ResetToHead
    } else {
        match message_id.filter(|message_id| *message_id > 0) {
            Some(message_id) => CursorDisposition::Deferred { message_id },
            None => CursorDisposition::Unchanged,
        }
    }
}
