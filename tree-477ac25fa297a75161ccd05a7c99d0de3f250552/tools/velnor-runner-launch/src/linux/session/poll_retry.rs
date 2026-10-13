//! Bounded retry of the side-effect-free message queue read.

use std::time::{Duration, Instant};
use std::{future::Future, pin::Pin};

use tokio::sync::watch;
use tokio::time::sleep;
use velnor_runner_github::policy::PollWithTrust;
use velnor_runner_github::{RefreshGate, SessionError};
use velnor_runner_journal::journal::Journal;

use super::{
    ActiveSession, LinuxLaunchContext, ShutdownGate, cutoff, observe_cutoff_for, poll_once,
};

const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
];

pub(super) type PollFuture<'a> = Pin<Box<dyn Future<Output = PollAttempt> + Send + 'a>>;
pub(super) type WaitFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// The production retry loop depends only on one poll attempt and one
/// shutdown-aware delay, so tests can script the protocol boundary.
pub(super) trait PollRetryIo {
    fn poll(&mut self, cursor: i64) -> PollFuture<'_>;
    fn wait(&mut self, delay: Duration) -> WaitFuture<'_>;
}

pub(super) struct SessionPollIo<'a> {
    pub(super) context: &'a LinuxLaunchContext,
    pub(super) journal: &'a Journal,
    pub(super) active: &'a ActiveSession,
    pub(super) shutdown: &'a mut watch::Receiver<Option<Instant>>,
    pub(super) cutoff: &'a mut Option<Instant>,
}

impl PollRetryIo for SessionPollIo<'_> {
    fn poll(&mut self, cursor: i64) -> PollFuture<'_> {
        Box::pin(poll_once(
            self.context,
            self.journal,
            self.active,
            cursor,
            self.shutdown,
            self.cutoff,
        ))
    }

    fn wait(&mut self, delay: Duration) -> WaitFuture<'_> {
        Box::pin(wait_before_next_poll(
            self.journal,
            self.shutdown,
            self.cutoff,
            self.context.drain_timeout(),
            delay,
        ))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PollAttempt {
    Delivered(PollWithTrust),
    RetryableUncertain,
    Terminal,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PollStep {
    Delivered(PollWithTrust),
    RetryAfter(Duration),
    Stop,
}

#[derive(Default)]
pub(super) struct PollRetryState {
    retries: usize,
}

impl PollRetryState {
    pub(super) fn next(&mut self, attempt: PollAttempt) -> PollStep {
        match attempt {
            PollAttempt::Delivered(poll) => PollStep::Delivered(poll),
            PollAttempt::RetryableUncertain => {
                let Some(delay) = RETRY_DELAYS.get(self.retries).copied() else {
                    return PollStep::Stop;
                };
                self.retries += 1;
                PollStep::RetryAfter(delay)
            }
            PollAttempt::Terminal => PollStep::Stop,
        }
    }
}

/// Run a bounded retry sequence at a fixed cursor. The cursor is advanced by
/// the caller only after this function returns a delivered batch.
pub(super) async fn poll_until_delivery<I: PollRetryIo>(
    io: &mut I,
    cursor: i64,
) -> Option<PollWithTrust> {
    let mut retries = PollRetryState::default();
    loop {
        match retries.next(io.poll(cursor).await) {
            PollStep::Delivered(poll) => return Some(poll),
            PollStep::RetryAfter(delay) if io.wait(delay).await => {}
            PollStep::RetryAfter(_) | PollStep::Stop => return None,
        }
    }
}

pub(super) fn classify_poll_result(
    result: Result<PollWithTrust, SessionError>,
    gate: &RefreshGate,
) -> PollAttempt {
    match result {
        Ok(poll) => PollAttempt::Delivered(poll),
        // A 401 starts an admin PATCH; uncertainty after it is not replayed here.
        Err(SessionError::Uncertain) if matches!(gate.started(), Ok(0)) => {
            PollAttempt::RetryableUncertain
        }
        Err(_) => PollAttempt::Terminal,
    }
}

pub(super) async fn wait_before_next_poll(
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
    drain_timeout: Duration,
    delay: Duration,
) -> bool {
    let mut gate = ShutdownGate {
        receiver: shutdown,
        cutoff,
    };
    let completed =
        cutoff::bounded_persisting(journal, &mut gate, drain_timeout, None, sleep(delay)).await;
    observe_cutoff_for(journal, gate.receiver, gate.cutoff, drain_timeout).await;
    completed.is_some() && gate.cutoff.is_none()
}
