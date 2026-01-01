//! One session loop. A running owned worker keeps the session up.

use std::future::Future;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::Journal;
use crate::listen::{Link, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::capacity::{self, Admit};
use super::slot;
use super::steps;
use super::trace;
use super::{Ready, ack_ready, drive_ready, scale_session};

/// Poll until admission stops and no owned launch container is running.
///
/// The target equals capacity unless `VELNOR_ADMIT_TARGET` is higher.
/// Capacity 1 still decides to stop after the first start. The session stays
/// while that container is running, and this does not remove it.
/// A full slot skips the statistics mint so restart does not start a second worker.
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
) -> Result<Vec<Started>, EnsureError> {
    trace::session(session);
    let mut workers = Vec::new();
    let capacity = capacity::job_capacity();
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    if let Some(started) = scale_if_free(journal, docker, capacity, population, || {
        scale_session(link, set_id, session, admin_token, journal, docker)
    })
    .await?
    {
        workers.push(started);
    }
    let target = capacity::admit_target(capacity);
    let mut turn = Turn {
        link,
        set_id,
        session,
        admin_token,
        journal,
        docker,
        capacity,
        target,
    };
    let bound = if target > capacity {
        capacity::poll_bound_wide()
    } else {
        capacity::poll_bound(capacity)
    };
    until_idle(&mut turn, &mut workers, bound).await?;
    Ok(workers)
}

pub(super) async fn scale_if_free<S, F>(
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
    population: i64,
    scale: S,
) -> Result<Option<Started>, EnsureError>
where
    S: FnOnce() -> F,
    F: Future<Output = Result<Option<Started>, EnsureError>>,
{
    let Ok(assigned) = u64::try_from(population) else {
        return Ok(None);
    };
    if assigned == 0 {
        return Ok(None);
    }
    let running = slot::running_count(journal, docker).await?;
    if running >= capacity || u64::from(running) >= assigned {
        return Ok(None);
    }
    scale().await
}

/// Keep polling after admission stops while an owned container is running.
async fn until_idle(
    turn: &mut Turn<'_>,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    let mut polls = 0usize;
    let mut missed = 0u8;
    loop {
        if polls >= bound && departed(turn, workers, missed).await? {
            return Ok(());
        }
        let stop = turn.drive_poll(workers).await?;
        polls = polls.saturating_add(1);
        if !stop {
            continue;
        }
        let running = slot::running_count(turn.journal, turn.docker).await?;
        if running > 0 {
            missed = 0;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }
        missed = missed.saturating_add(1);
        if workers.is_empty() || missed >= 2 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn departed(turn: &Turn<'_>, workers: &[Started], missed: u8) -> Result<bool, EnsureError> {
    let running = slot::running_count(turn.journal, turn.docker).await?;
    Ok(running == 0 && (workers.is_empty() || missed >= 2))
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

struct Turn<'a> {
    link: &'a mut Link,
    set_id: i64,
    session: &'a QueueSession,
    admin_token: &'a str,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    capacity: u32,
    target: u32,
}

impl Turn<'_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        let polled = poll_path(self.link, self.session, &path, self.capacity);
        restore_base(self.link, saved)?;
        let polled = polled?;
        trace::batch(&polled);
        let idle = steps::idle(&polled);
        let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
        let running = self.running(started, idle).await?;
        let decision = capacity::admit(capacity::Seat {
            capacity: self.capacity,
            target: self.target,
            started,
            running,
            assigned: assigned_in(&polled),
            idle,
        });
        self.apply(decision, workers, path, queue, &polled).await
    }

    async fn running(&self, started: u32, idle: steps::Idle) -> Result<u32, EnsureError> {
        if !capacity::needs_running(self.capacity, self.target, started, idle) {
            return Ok(0);
        }
        slot::running_count(self.journal, self.docker).await
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
                ack_ready(self.link, self.session, path, queue, polled)?;
                Ok(stop)
            }
            Admit::Start { stop } => {
                let launched = drive_ready(
                    self.link,
                    Ready {
                        set_id: self.set_id,
                        session: self.session,
                        admin_token: self.admin_token,
                        path,
                        queue,
                        polled,
                    },
                    self.journal,
                    self.docker,
                    self.capacity,
                )
                .await?;
                let Some(worker) = launched else {
                    return Ok(false);
                };
                workers.push(worker);
                Ok(stop)
            }
        }
    }

    async fn stay(&self, workers: &[Started]) -> Result<bool, EnsureError> {
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
