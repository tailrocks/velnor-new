//! One session loop. A running owned worker keeps the session up.

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
    slot::release_exited(journal, docker).await?;
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    let occupied = slot::occupied(journal).await?;
    let running = slot::running_count(journal, docker).await?;
    if !capacity::statistics_blocked(occupied, running, capacity, population)
        && let Some(started) =
            scale_session(link, set_id, session, admin_token, journal, docker).await?
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

/// One session's poll and running-count source.
trait PollHost {
    /// `Ok(false)` keeps the session. `Ok(true)` is an admission stop.
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError>;

    /// Owned containers still running.
    async fn running(&mut self) -> Result<u32, EnsureError>;
}

impl PollHost for Turn<'_> {
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.drive_poll(workers).await
    }

    async fn running(&mut self) -> Result<u32, EnsureError> {
        slot::running_count(self.journal, self.docker).await
    }
}

/// Keep polling while an owned container runs. An empty session stays open past `bound`.
async fn until_idle(
    turn: &mut Turn<'_>,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    pump(turn, workers, bound).await
}

async fn pump<H: PollHost>(
    host: &mut H,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    let mut polls = 0usize;
    let mut missed = 0u8;
    loop {
        if polls >= bound && !workers.is_empty() && missed >= 2 && host.running().await? == 0 {
            return Ok(());
        }
        let stop = host.poll(workers).await?;
        polls = polls.saturating_add(1);
        if !stop {
            // The broker can assign a job only while this session still exists.
            if workers.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            continue;
        }
        if host.running().await? > 0 {
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
    let occupied = slot::occupied(journal).await?;
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
        start_turn(
            &mut lane,
            workers,
            Ready {
                set_id: self.set_id,
                session: self.session,
                admin_token: self.admin_token,
                path,
                polled,
            },
            self.journal,
            self.docker,
            self.capacity,
            stop,
        )
        .await
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

/// Start one admitted poll through an injectable lane. A conflict remains an
/// error so the current message is redelivered; only a bound worker covers it.
async fn start_turn<T>(
    lane: &mut T,
    workers: &mut Vec<Started>,
    ready: Ready<'_>,
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
    stop: bool,
) -> Result<bool, EnsureError>
where
    T: velnor_runner_github::Transport + super::Lane,
{
    let Some(worker) = drive_ready(lane, ready, journal, docker, capacity).await? else {
        return Ok(false);
    };
    workers.push(worker);
    Ok(stop)
}

#[cfg(test)]
mod tests;
