// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ops::AsyncFnMut;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation};
use crate::launch::FreshPressureSample;
use crate::listen::Link;
use crate::scale_set::EnsureError;

pub(crate) struct PositivePollContext<'a> {
    pub(crate) link: &'a mut Link,
    pub(crate) set_id: i64,
    pub(crate) session: &'a mut QueueSession,
    pub(crate) scoped: &'a ConnectionJournal,
    pub(crate) session_intent_id: i64,
    pub(crate) allocation: &'a HeaderAllocation,
    pub(crate) cursor: i64,
}

use super::completion_intake::completion_error;
use super::steps::{AcquiredProvisioning, KnownNoAcquire};
use super::{ack_ready, poll_scoped_queue};

pub(crate) enum PositiveAdmission {
    NoAcquire,

    Deferred(crate::launch::DeferredAuthorizedOffer),

    Acquired(Box<AcquiredProvisioning>),

    KnownNoAcquire(KnownNoAcquire),

    OwnedTerminal(OwnedTerminalReplay),
}

#[path = "positive_poll/owned_terminal.rs"]
pub(crate) mod owned_terminal;
pub(crate) use owned_terminal::{OwnedTerminalReplay, classify_owned_terminal_available};

pub(crate) struct PositivePollOutcome<A = PositiveAdmission> {
    pub(crate) assigned_total: Option<i64>,

    pub(crate) message_id: Option<i64>,

    pub(crate) cursor_disposition: CursorDisposition,

    pub(crate) started_request_ids: Vec<i64>,

    pub(crate) wake_cleanup: bool,

    pub(crate) session_delete_candidate: bool,

    pub(crate) admission: Option<A>,

    #[expect(
        dead_code,
        reason = "retains ACK uncertainty asserted by adverse replay tests while production uses cursor disposition and settlement"
    )]
    pub(crate) acknowledgement: Acknowledgement,

    pub(crate) settlement: Settlement,

    pub(crate) observation: crate::launch::steady::PollObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorDisposition {
    Unchanged,

    ResetToHead,

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
pub(crate) use controller::positive_poll_once_with_pressure;

async fn begin_poll(
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    session: &QueueSession,
    allocation: &HeaderAllocation,
    capacity_mode: PollCapacityMode,
) -> Result<crate::journal::HeaderPollAttempt, EnsureError> {
    if allocation.target() == 0
        || !allocation.matches_session(scoped.connection_id(), set_id, &session.session_id)
    {
        return Err(EnsureError::AdmissionClosed);
    }
    let attempt = match capacity_mode {
        PollCapacityMode::Pressure(sample) => {
            scoped
                .begin_session_pressure_poll(session_intent_id, allocation, sample)
                .await
        }
    }
    .map_err(|_| completion_error())?;
    let header_valid = match capacity_mode {
        PollCapacityMode::Pressure(_) => attempt.header_value() <= allocation.target(),
    };
    if !header_valid || attempt.header_value() == 0 {
        scoped
            .mark_session_poll_uncertain(session_intent_id, attempt)
            .await
            .map_err(|_| completion_error())?;
        return Err(EnsureError::AdmissionClosed);
    }
    Ok(attempt)
}

#[derive(Clone, Copy)]
enum PollCapacityMode {
    Pressure(Option<FreshPressureSample>),
}

struct AdmissionContext<'a> {
    link: &'a mut Link,
    session: &'a QueueSession,
    scoped: &'a ConnectionJournal,
    session_intent_id: i64,
    attempt: crate::journal::HeaderPollAttempt,
}

async fn invoke_admission<F>(
    context: AdmissionContext<'_>,
    polled: &Poll,
    queue: Option<String>,
    path: String,
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
    let AdmissionContext {
        link,
        session,
        scoped,
        session_intent_id,
        attempt,
    } = context;
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
