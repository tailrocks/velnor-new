//! One session loop. A running owned worker keeps the session up.

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::Journal;
use crate::listen::{Link, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;
use crate::worker::{ResourceBudget, Started};

use super::capacity::{self, Admit};
use super::completion::CompletionWorker;
use super::slot;
use super::steps;
use super::trace;
use super::{Ready, Rest, ack_ready, drive_ready, scale_session};

mod completion_intake;
mod pump;
use completion_intake::{completion_error, intake_and_ack_if_only};
use pump::until_idle;
#[cfg(all(test, unix))]
use pump::{PollHost, pump};

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
    let capacity = super::pressure::advertise(ceiling);
    let resource_budget = rest.resource_budget.ok_or(EnsureError::Unexpected {
        status: 0,
        step: "resource budget",
    })?;
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    let occupied = slot::occupied(journal).await?;
    let running = slot::running_count(journal, docker).await?;
    if !capacity::statistics_blocked(occupied, running, capacity, population)
        && let Some(started) =
            scale_session(link, set_id, session, admin_token, journal, docker, rest).await?
    {
        workers.push(started);
    }
    let completion = CompletionWorker::start(
        journal.clone(),
        docker.clone(),
        link.base(),
        admin_token.to_owned(),
    )
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
        owner: rest.owner,
        repo: rest.repo,
        pat: rest.pat,
        resource_budget,
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
        Some(EnsureError::NameSteady | EnsureError::NameCleared)
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
    owner: &'a str,
    repo: &'a str,
    pat: &'a str,
    resource_budget: ResourceBudget,
    cursor: i64,
    steady_retry: Option<std::time::Instant>,
}

impl Turn<'_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.fit_pressure().await?;
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        let now = std::time::Instant::now();
        self.cursor = super::steady::poll_cursor(self.cursor, self.steady_retry, now);
        let polled = poll_path(self.link, self.session, &path, self.capacity, self.cursor);
        restore_base(self.link, saved)?;
        let polled = polled?;
        trace::batch(&polled);
        let link = &mut *self.link;
        let session = self.session;
        let set_id = self.set_id;
        let journal = self.journal;
        let ack_path = path.clone();
        let ack_queue = queue.clone();
        let ack_polled = &polled;
        let intake = intake_and_ack_if_only(journal, set_id, &polled, || async move {
            ack_ready(link, session, ack_path, ack_queue, ack_polled)
        })
        .await?;
        if intake.wake_cleanup {
            self.completion.notify();
        }
        if intake.completion_only {
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
        let ceiling = capacity::job_capacity();
        let next = super::pressure::adjust(self.capacity, running_now, ceiling);
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
        let rest = Rest {
            owner: self.owner,
            repo: self.repo,
            pat: self.pat,
            resource_budget: Some(self.resource_budget),
        };
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
