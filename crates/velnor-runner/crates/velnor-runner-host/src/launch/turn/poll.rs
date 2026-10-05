//! Apply one queue result through the listener's production admission path.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use velnor_runner_github::Poll;

use crate::journal::{Journal, LaunchReservation};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::{PollAdmission, admission};
use crate::launch::capacity::Admit;

#[cfg(test)]
mod tests;

pub(in crate::launch) type DispatchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Started>, EnsureError>> + 'a>>;

/// The queue actions selected by one production poll.
pub(in crate::launch) trait PollDispatcher {
    /// Acknowledge a batch after admission allows it.
    fn ack(
        &mut self,
        path: String,
        queue: Option<String>,
        polled: &Poll,
    ) -> Result<(), EnsureError>;

    /// Acquire, prepare, and start one reserved assignment.
    fn start<'a>(
        &'a mut self,
        journal: &'a Journal,
        path: String,
        queue: Option<String>,
        polled: &'a Poll,
        reservation: Option<LaunchReservation>,
    ) -> DispatchFuture<'a>;
}

pub(in crate::launch) type LoopFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, EnsureError>> + 'a>>;

/// Operations used by the listener's bounded wait loop.
pub(in crate::launch) trait LoopDriver {
    /// Poll and apply one queue response.
    fn drive<'a>(&'a mut self, workers: &'a mut Vec<Started>) -> LoopFuture<'a, bool>;

    /// Count workers that are running now.
    fn running<'a>(&'a mut self) -> LoopFuture<'a, u32>;

    /// Check for durable completion cleanup that still owns a slot.
    fn has_pending_cleanup<'a>(&'a mut self) -> LoopFuture<'a, bool>;

    /// Wait between polls when a worker or cleanup is still active.
    fn pause<'a>(&'a mut self, duration: Duration) -> LoopFuture<'a, ()>;
}

/// Keep polling through active work and cleanup, then leave after two idle notices.
pub(in crate::launch) async fn until_idle<D: LoopDriver>(
    turn: &mut D,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    let mut polls = 0usize;
    let mut missed = 0u8;
    loop {
        if polls >= bound && departed(turn, workers, missed).await? {
            return Ok(());
        }
        let stop = turn.drive(workers).await?;
        polls = polls.saturating_add(1);
        if !stop {
            continue;
        }
        if turn.running().await? > 0 || turn.has_pending_cleanup().await? {
            missed = 0;
            turn.pause(Duration::from_secs(2)).await?;
            continue;
        }
        missed = missed.saturating_add(1);
        if workers.is_empty() || missed >= 2 {
            return Ok(());
        }
        turn.pause(Duration::from_secs(2)).await?;
    }
}

async fn departed<D: LoopDriver>(
    turn: &mut D,
    workers: &[Started],
    missed: u8,
) -> Result<bool, EnsureError> {
    let running = turn.running().await?;
    let pending = turn.has_pending_cleanup().await?;
    Ok(running == 0 && !pending && (workers.is_empty() || missed >= 2))
}

/// Apply the same admission and dispatch logic used by the listener.
pub(in crate::launch) async fn apply<D: PollDispatcher>(
    journal: &Journal,
    set_id: i64,
    capacity_limit: u32,
    target: u32,
    workers: &mut Vec<Started>,
    running: u32,
    polled: &Poll,
    path: String,
    queue: Option<String>,
    dispatcher: &mut D,
) -> Result<bool, EnsureError> {
    let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
    let decision = admission(
        journal,
        set_id,
        capacity_limit,
        target,
        started,
        running,
        polled,
    )
    .await?;
    dispatch(
        decision,
        target,
        capacity_limit,
        workers,
        journal,
        polled,
        path,
        queue,
        dispatcher,
    )
    .await
}

async fn dispatch<D: PollDispatcher>(
    admission: PollAdmission,
    target: u32,
    capacity_limit: u32,
    workers: &mut Vec<Started>,
    journal: &Journal,
    polled: &Poll,
    path: String,
    queue: Option<String>,
    dispatcher: &mut D,
) -> Result<bool, EnsureError> {
    match admission.decision {
        Admit::Stay => {
            if target > capacity_limit && workers.len() >= capacity_limit as usize {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            Ok(false)
        }
        Admit::Hold => {
            if target > capacity_limit {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                Ok(false)
            } else {
                Ok(true)
            }
        }
        Admit::Stop => Ok(true),
        Admit::Error => Err(EnsureError::Unexpected {
            status: 0,
            step: "queue",
        }),
        Admit::Ack { stop } => {
            dispatcher.ack(path, queue, polled)?;
            Ok(stop)
        }
        Admit::Start { stop } => {
            let launched = dispatcher
                .start(journal, path, queue, polled, admission.reservation)
                .await?;
            let Some(worker) = launched else {
                return Ok(false);
            };
            workers.push(worker);
            Ok(stop)
        }
    }
}
