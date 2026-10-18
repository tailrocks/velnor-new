//! One session loop. A running owned worker keeps the session up.

use std::future::Future;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{ConnectionJournal, HeaderAllocation, HeaderPollAttempt, Journal};
use crate::listen::{Link, PollPathResult, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::capacity::{self, Admit};
use super::completion::CompletionWorker;
use super::slot;
use super::steps;
use super::trace;
use super::{Ready, Rest, ack_ready, drive_ready};

mod completion_intake;
mod pump;
use completion_intake::{
    IntakeDecision, completion_error, intake_and_ack_if_only, intake_and_ack_if_only_scoped,
};
use pump::until_idle;
#[cfg(all(test, unix))]
use pump::{PollHost, pump};

/// Poll one scoped session at a zero header and persist lifecycle events before ACK.
///
/// This path never acquires an Available job or starts a worker. Positive
/// capacity requires the separate fresh-permit admission path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScopedPollOutcome {
    pub(crate) wake_cleanup: bool,
    pub(crate) session_delete_candidate: bool,
}

pub(crate) async fn poll_scoped_connection_once(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> Result<ScopedPollOutcome, EnsureError> {
    if !allocation.matches_session(scoped.connection_id(), set_id, &session.session_id) {
        return Err(EnsureError::AdmissionClosed);
    }
    let attempt = scoped
        .begin_zero_header_poll(session_intent_id, allocation)
        .await
        .map_err(|_| completion_error())?;
    let (polled, path, queue) =
        match poll_scoped_queue(link, session, scoped, session_intent_id, set_id, 0, 0).await {
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
        attempt,
        handled,
    )
    .await
}

async fn poll_scoped_queue(
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
            let refreshed = super::session::refresh_bound_connection_session(
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

async fn finish_scoped_poll_response<F>(
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    session_id: &str,
    allocation: &HeaderAllocation,
    attempt: HeaderPollAttempt,
    handled: F,
) -> Result<ScopedPollOutcome, EnsureError>
where
    F: Future<Output = Result<IntakeDecision, EnsureError>>,
{
    let decision = match handled.await {
        Ok(decision) => decision,
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            return Err(error);
        }
    };
    let completed = super::session::CompletedHeaderPoll::after_response(
        attempt, allocation, scoped, set_id, session_id,
    )?;
    scoped
        .settle_session_poll(session_intent_id, completed)
        .await
        .map_err(|_| completion_error())?;
    Ok(ScopedPollOutcome {
        wake_cleanup: decision.wake_cleanup,
        session_delete_candidate: decision.session_delete_candidate,
    })
}

/// Poll until admission stops and no owned launch container is running.
///
/// # Errors
///
/// Returns [`EnsureError`] when one message carries two ids, or a later step fails.
pub(super) async fn poll_and_drive(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    admin_token: &str,
    journal: &Journal,
    docker: &bollard::Docker,
    rest: Rest<'_>,
) -> Result<Vec<Started>, EnsureError> {
    trace::session(session);
    let mut workers = Vec::new();
    let ceiling = capacity::job_capacity();
    let capacity = if rest.scoped_journal.is_some() {
        super::pressure::advertise_allocated(rest.allocated_header)
    } else {
        super::pressure::advertise(ceiling)
    };
    if rest.resource_budget.is_none() {
        return Err(EnsureError::Unexpected {
            status: 0,
            step: "resource budget",
        });
    }
    let completion = match rest.scoped_journal {
        Some(scoped) => CompletionWorker::start_scoped(
            scoped.clone(),
            docker.clone(),
            link.base(),
            admin_token.to_owned(),
        ),
        None => CompletionWorker::start(
            journal.clone(),
            docker.clone(),
            link.base(),
            admin_token.to_owned(),
        ),
    }
    .map_err(|_| completion_error())?;
    let target = capacity::admit_target(capacity);
    let mut turn = Turn {
        link,
        set_id,
        session,
        admin_token,
        journal,
        docker,
        completion: &completion,
        capacity,
        target,
        rest,
        cursor: 0,
        steady_retry: None,
    };
    let bound = if target > capacity {
        capacity::poll_bound_wide()
    } else {
        capacity::poll_bound(capacity)
    };
    let driven = until_idle(&mut turn, &mut workers, bound).await;
    let stopped = completion.shutdown().await;
    match (driven, stopped) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(_)) => Err(completion_error()),
        (Ok(()), Ok(())) => Ok(workers),
    }
}

/// A cleared job name and a steady scale name stay on the queue.
///
/// Neither error acknowledges the message or ends the session.
fn queue_stays(error: Option<&EnsureError>) -> bool {
    matches!(
        error,
        Some(
            EnsureError::NameSteady
                | EnsureError::NameCleared
                | EnsureError::SourceBlocked { .. }
                | EnsureError::AdmissionClosed
        )
    )
}

fn progress_batch(polled: &Poll) -> bool {
    let Poll::Batch(batch) = polled else {
        return false;
    };
    crate::assign::progress_only(batch)
}

fn assigned_in(polled: &Poll) -> u32 {
    let Poll::Batch(batch) = polled else {
        return 0;
    };
    let raw = batch
        .statistics
        .as_ref()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    u32::try_from(raw.max(0)).unwrap_or(u32::MAX)
}

pub(crate) async fn admission<E: crate::stage::PairEngine + ?Sized>(
    engine: &E,
    journal: &Journal,
    capacity: u32,
    target: u32,
    started: u32,
    polled: &Poll,
) -> Result<Admit, EnsureError> {
    slot::release_exited(journal, engine).await?;
    let idle = steps::idle(polled);
    let except = steps::mint_subject(polled);
    let occupied = slot::occupied_except(journal, except.as_deref()).await?;
    let running = if capacity::needs_running(idle) {
        slot::running_count(journal, engine).await?
    } else {
        0
    };
    Ok(capacity::admit(capacity::Seat {
        capacity,
        target,
        started,
        occupied,
        running,
        assigned: assigned_in(polled),
        idle,
        progress: progress_batch(polled),
    }))
}

struct Turn<'a> {
    link: &'a mut Link,
    set_id: i64,
    session: &'a QueueSession,
    admin_token: &'a str,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    completion: &'a CompletionWorker,
    capacity: u32,
    target: u32,
    rest: super::drive::Rest<'a>,
    cursor: i64,
    steady_retry: Option<std::time::Instant>,
}

impl Turn<'_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.rest.source_authorized = false;
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        let now = std::time::Instant::now();
        self.cursor = super::steady::poll_cursor(self.cursor, self.steady_retry, now);
        let polled = poll_path(self.link, self.session, &path, self.capacity, self.cursor);
        restore_base(self.link, saved)?;
        let polled = match polled? {
            PollPathResult::Response(polled) => polled,
            // The legacy positive turn has no mutable, durable session scope.
            // Leave this path closed rather than replaying a stale queue GET.
            PollPathResult::RefreshRequired => return Err(EnsureError::AdmissionClosed),
        };
        trace::batch(&polled);
        let link = &mut *self.link;
        let session = self.session;
        let set_id = self.set_id;
        let journal = self.journal;
        let ack_path = path.clone();
        let ack_queue = queue.clone();
        let ack_polled = &polled;
        let intake = if let Some(scoped) = self.rest.scoped_journal {
            intake_and_ack_if_only_scoped(scoped, set_id, &polled, || async move {
                ack_ready(link, session, ack_path, ack_queue, ack_polled)
            })
            .await?
        } else {
            intake_and_ack_if_only(journal, set_id, &polled, || async move {
                ack_ready(link, session, ack_path, ack_queue, ack_polled)
            })
            .await?
        };
        if intake.wake_cleanup {
            self.completion.notify();
        }
        // Record lifecycle events before source authorization can reject an
        // offered job in the same batch. Mixed batches stay unacknowledged,
        // while their exact Started/Completed events remain durable.
        if self.rest.permits_start() {
            super::steps::authorize_poll_rest(&self.rest, &polled).await?;
            self.rest.source_authorized = true;
            self.fit_pressure().await?;
        }
        if intake.completion_only || !self.rest.permits_start() {
            return Ok(false);
        }
        let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
        let decision = admission(
            self.docker,
            self.journal,
            self.capacity,
            self.target,
            started,
            &polled,
        )
        .await?;
        self.apply(decision, workers, path, queue, &polled).await
    }

    async fn fit_pressure(&mut self) -> Result<(), EnsureError> {
        let running_now = slot::running_count(self.journal, self.docker).await?;
        let ceiling = if self.rest.scoped_journal.is_some() {
            self.rest.allocated_header
        } else {
            capacity::job_capacity()
        };
        let host_journal = self.rest.host_journal.unwrap_or(self.journal);
        let sample = match self.rest.release_read_secret {
            Some(secret) => {
                super::resource_probe::pressure_sample(self.docker, host_journal, secret).await
            }
            None => None,
        };
        let next = super::pressure::adjust_with_sample(self.capacity, running_now, ceiling, sample);
        if self.target == self.capacity {
            self.target = next;
        }
        self.capacity = next;
        Ok(())
    }

    async fn apply(
        &mut self,
        decision: Admit,
        workers: &mut Vec<Started>,
        path: String,
        queue: Option<String>,
        polled: &Poll,
    ) -> Result<bool, EnsureError> {
        match decision {
            // HTTP 202 keeps the session open. A job can arrive on a later poll.
            Admit::Stay => self.stay(workers).await,
            Admit::Hold | Admit::Start { .. } if !self.rest.permits_start() => {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                Ok(false)
            }
            Admit::Hold => self.hold().await,
            Admit::Stop => Ok(true),
            Admit::Error => Err(EnsureError::Unexpected {
                status: 0,
                step: "queue",
            }),
            Admit::Ack { stop } => {
                // Fail only an unstarted `m{id}` row. A container row stays.
                super::name_taken::fail_unstarted(self.journal, polled).await?;
                ack_ready(self.link, self.session, path, queue, polled)?;
                Ok(stop)
            }
            Admit::Start { stop } => self.start(workers, path, queue, polled, stop).await,
        }
    }

    async fn start(
        &mut self,
        workers: &mut Vec<Started>,
        path: String,
        queue: Option<String>,
        polled: &Poll,
        stop: bool,
    ) -> Result<bool, EnsureError> {
        let admin = self.link.base().to_owned();
        let mut lane = super::HostLane {
            link: self.link,
            admin,
            queue: queue.clone(),
        };
        let rest: Rest<'_> = self.rest;
        let launched = drive_ready(
            &mut lane,
            Ready {
                set_id: self.set_id,
                session: self.session,
                admin_token: self.admin_token,
                path: path.clone(),
                polled,
            },
            self.journal,
            self.docker,
            self.capacity,
            rest,
        )
        .await;
        if queue_stays(launched.as_ref().err()) {
            // Skip this message id. Do not delete that runner again and do not ack.
            let id = super::steady::message_id(polled);
            if let Some(next) = super::steady::steady_cursor(self.cursor, id) {
                self.cursor = next;
                self.steady_retry = Some(std::time::Instant::now() + super::steady::RETRY);
                return Ok(false);
            }
            tokio::time::sleep(super::steady::RETRY).await;
            return Ok(false);
        }
        if let Err(EnsureError::Conflict) = &launched
            && super::name_taken::should_ack(steps::idle(polled))
        {
            // A container row stays. The message is still acknowledged.
            // A new mint of the same name cannot succeed.
            super::name_taken::fail_unstarted(self.journal, polled).await?;
            ack_ready(self.link, self.session, path, queue, polled)?;
            return Ok(false);
        }
        let Some(worker) = launched? else {
            return Ok(false);
        };
        workers.push(worker);
        Ok(stop)
    }

    async fn stay(&self, workers: &[Started]) -> Result<bool, EnsureError> {
        if super::steady::pause_empty(self.cursor) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        if self.target > self.capacity && workers.len() >= self.capacity as usize {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        Ok(false)
    }

    async fn hold(&self) -> Result<bool, EnsureError> {
        if self.target > self.capacity {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            return Ok(false);
        }
        Ok(true)
    }
}

/// Test seam for one admitted poll through an injectable lane.
///
/// A conflict remains an error here; production handling for cleared and
/// colliding names lives in [`Turn::start`]. Only a bound worker covers it.
#[cfg(test)]
struct StartTurn<'a> {
    ready: Ready<'a>,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    capacity: u32,
    rest: Rest<'a>,
    stop: bool,
}

#[cfg(test)]
async fn start_turn<T>(
    lane: &mut T,
    workers: &mut Vec<Started>,
    turn: StartTurn<'_>,
) -> Result<bool, EnsureError>
where
    T: velnor_runner_github::Transport + super::Lane,
{
    let StartTurn {
        ready,
        journal,
        docker,
        capacity,
        rest,
        stop,
    } = turn;
    let Some(worker) = drive_ready(lane, ready, journal, docker, capacity, rest).await? else {
        return Ok(false);
    };
    workers.push(worker);
    Ok(stop)
}

#[cfg(all(test, unix))]
mod progress_tests;
#[cfg(all(test, unix))]
mod start_idless_tests;
#[cfg(all(test, unix))]
mod start_jit_rejection_tests;
#[cfg(all(test, unix))]
mod start_tests;

#[cfg(test)]
#[path = "turn/completion_intake_tests.rs"]
mod completion_intake_tests;

#[cfg(test)]
#[path = "turn/completion_quarantine_tests.rs"]
mod completion_quarantine_tests;

#[cfg(test)]
#[path = "turn/scoped_poll_tests.rs"]
mod scoped_poll_tests;
