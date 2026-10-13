//! One session loop. A running owned worker keeps the session up.

use std::future::Future;

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{Journal, LaunchReservation};
use crate::listen::{Link, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::capacity::{self, Admit};
use super::capacity::{CapacityHysteresis, GuestResourceLimits};
use super::completion;
use super::scale_session;
use super::slot;
use super::steps;
use super::trace;

mod poll;
mod production;

#[cfg(test)]
mod admission_tests;

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
    let configured_capacity = capacity::job_capacity();
    let initial_occupied = slot::occupied_count(journal).await?;
    let mut capacity_policy = CapacityHysteresis::new();
    let initial_capacity = capacity_policy.update(
        configured_capacity,
        GuestResourceLimits::default(),
        initial_occupied,
    );
    let capacity = initial_capacity.ceiling;
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    if let Some(started) = scale_if_free_with_occupancy(
        journal,
        docker,
        capacity,
        population,
        initial_capacity.occupied,
        || scale_session(link, set_id, session, admin_token, journal, docker),
    )
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
        configured_capacity,
        capacity_policy,
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
    let occupied = slot::occupied_count(journal).await?;
    scale_if_free_with_occupancy(journal, docker, capacity, population, occupied, scale).await
}

async fn scale_if_free_with_occupancy<S, F>(
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
    population: i64,
    occupied: u32,
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
    if occupied >= capacity {
        return Ok(None);
    }
    let running = slot::running_count(journal, docker).await?;
    if occupied > running || running >= capacity || u64::from(running) >= assigned {
        return Ok(None);
    }
    scale().await
}

/// Admission result plus any launch reservation made before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PollAdmission {
    pub(super) decision: Admit,
    pub(super) reservation: Option<LaunchReservation>,
}

/// Reserve an offered assignment before applying the slot gate.
pub(super) async fn admission(
    journal: &Journal,
    set_id: i64,
    capacity: u32,
    target: u32,
    started: u32,
    running: u32,
    polled: &Poll,
) -> Result<PollAdmission, EnsureError> {
    if let Some((batch, request_id)) = steps::assignment(polled)? {
        let reservation = journal
            .reserve_assignment(set_id, request_id, batch.message_id, capacity)
            .await
            .map_err(slot::map_journal)?;
        let decision = match reservation {
            LaunchReservation::AtCapacity => Admit::Hold,
            LaunchReservation::Completed(_) => Admit::Ack { stop: false },
            LaunchReservation::New(_) | LaunchReservation::Existing(_) => {
                let occupied = slot::occupied_count(journal).await?;
                Admit::Start {
                    stop: occupied >= capacity,
                }
            }
        };
        return Ok(PollAdmission {
            decision,
            reservation: Some(reservation),
        });
    }
    let occupied = slot::occupied_count(journal).await?;
    Ok(PollAdmission {
        decision: capacity::admit(capacity::Seat {
            capacity,
            target,
            started,
            occupied,
            running,
            assigned: assigned_in(polled),
            idle: steps::idle(polled),
        }),
        reservation: None,
    })
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
        if !turn
            .journal
            .completed_launches()
            .await
            .map_err(slot::map_journal)?
            .is_empty()
        {
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
    let pending = turn
        .journal
        .completed_launches()
        .await
        .map_err(slot::map_journal)?;
    Ok(running == 0 && pending.is_empty() && (workers.is_empty() || missed >= 2))
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
    configured_capacity: u32,
    capacity_policy: CapacityHysteresis,
    capacity: u32,
    target: u32,
}

impl Turn<'_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        let occupied = slot::occupied_count(self.journal).await?;
        let capacity = self.capacity_policy.update(
            self.configured_capacity,
            GuestResourceLimits::default(),
            occupied,
        );
        self.capacity = capacity.ceiling;
        self.target = capacity::admit_target(self.capacity);
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        let polled = poll_path(self.link, self.session, &path, self.capacity);
        restore_base(self.link, saved)?;
        let polled = polled?;
        trace::batch(&polled);
        completion::record_completion_events(self.journal, self.set_id, &polled).await?;
        let cleanup_tasks = completion::schedule_completed(
            self.link.transport().cleanup_client(),
            self.set_id,
            self.admin_token,
            self.journal.clone(),
            self.docker.clone(),
        )
        .await?;
        drop(cleanup_tasks);
        let idle = steps::idle(&polled);
        let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
        let running = self.running(started, idle).await?;
        let mut dispatcher = production::LinkDispatcher {
            link: self.link,
            set_id: self.set_id,
            session: self.session,
            admin_token: self.admin_token,
            docker: self.docker,
        };
        poll::apply(
            self.journal,
            self.set_id,
            self.capacity,
            self.target,
            workers,
            running,
            &polled,
            path,
            queue,
            &mut dispatcher,
        )
        .await
    }

    async fn running(&self, started: u32, idle: steps::Idle) -> Result<u32, EnsureError> {
        if idle != steps::Idle::Scale
            || !capacity::needs_running(self.capacity, self.target, started, idle)
        {
            return Ok(0);
        }
        slot::running_count(self.journal, self.docker).await
    }
}
