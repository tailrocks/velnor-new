//! Positive-capacity polling with durable attempt and acquire ownership.

use std::ops::AsyncFnMut;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation};
use crate::listen::Link;
use crate::scale_set::EnsureError;

#[cfg(test)]
use super::completion_intake::IntakeDecision;
use super::completion_intake::completion_error;
use super::completion_intake::intake_and_ack_if_only_scoped;
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
    /// The exact Available message belongs to a durable terminal request.
    OwnedTerminal(OwnedTerminalReplay),
}

#[path = "positive_poll/owned_terminal.rs"]
pub(crate) mod owned_terminal;
pub(crate) use owned_terminal::{OwnedTerminalReplay, classify_owned_terminal_available};

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

#[path = "positive_poll/controller.rs"]
mod controller;
pub(crate) use controller::positive_poll_once;
#[cfg(test)]
pub(crate) use controller::test_positive_poll_once_with_response;

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
use finish::contains_available;
#[cfg(test)]
use finish::finish_positive_poll;
#[cfg(test)]
pub(crate) use finish::finish_positive_poll_with_ack;
#[cfg(test)]
use finish::{
    acknowledgement_after_intake, admit_available, authorized_deferred_matches, cursor_disposition,
    make_outcome, settle_known_response,
};

#[cfg(test)]
#[path = "positive_poll_tests.rs"]
mod tests;
