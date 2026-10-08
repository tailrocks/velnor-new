//! Absolute stop-cutoff propagation for asynchronous session phases.

use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, sleep, sleep_until};

use super::{ShutdownGate, observe_cutoff_for};

const DRAIN_CHECK_INTERVAL: Duration = Duration::from_millis(250);

const DISPATCH_PENDING: u8 = 0;
const DISPATCH_CANCELLED: u8 = 1;
const DISPATCH_STARTED: u8 = 2;

/// One-shot permission for a blocking protocol call to begin its request.
#[derive(Clone)]
pub(in crate::linux) struct DispatchFence {
    state: Arc<AtomicU8>,
    cancelled: Arc<AtomicBool>,
}

impl DispatchFence {
    pub(in crate::linux) fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(DISPATCH_PENDING)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Stop queued work and cancel an in-flight bounded wire request.
    pub(super) fn cancel_pending(&self) {
        self.cancelled.store(true, Ordering::Release);
        match self.state.compare_exchange(
            DISPATCH_PENDING,
            DISPATCH_CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) | Err(_) => {}
        }
    }

    pub(super) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub(super) fn is_pending(&self) -> bool {
        self.state.load(Ordering::Acquire) == DISPATCH_PENDING
    }

    /// Return the current absolute wire cutoff, rejecting cancelled, closed,
    /// or already-expired phases before every request (including replay).
    pub(super) fn wire_deadline(
        &self,
        shutdown: &watch::Receiver<Option<Instant>>,
        phase_deadline: Option<Instant>,
    ) -> Result<Option<Instant>, ()> {
        let signal_deadline = *shutdown.borrow();
        if self.cancelled.load(Ordering::Acquire)
            || (shutdown.has_changed().is_err() && signal_deadline.is_none())
        {
            return Err(());
        }
        let deadline = earliest(phase_deadline, signal_deadline);
        if deadline.is_some_and(|value| Instant::now() >= value) {
            self.cancel_pending();
            Err(())
        } else {
            Ok(deadline)
        }
    }

    pub(super) fn allowed(
        &self,
        shutdown: &watch::Receiver<Option<Instant>>,
        phase_deadline: Option<Instant>,
    ) -> bool {
        self.is_pending()
            && !self.cancelled.load(Ordering::Acquire)
            && before_deadlines(shutdown, phase_deadline)
    }

    pub(super) fn begin(
        &self,
        shutdown: &watch::Receiver<Option<Instant>>,
        phase_deadline: Option<Instant>,
    ) -> bool {
        if !self.allowed(shutdown, phase_deadline)
            || self
                .state
                .compare_exchange(
                    DISPATCH_PENDING,
                    DISPATCH_STARTED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
        {
            return false;
        }
        self.allowed_started(shutdown, phase_deadline)
    }

    fn allowed_started(
        &self,
        shutdown: &watch::Receiver<Option<Instant>>,
        phase_deadline: Option<Instant>,
    ) -> bool {
        if self.cancelled.load(Ordering::Acquire) || !before_deadlines(shutdown, phase_deadline) {
            self.cancel_pending();
            return false;
        }
        true
    }
}

fn before_deadlines(
    shutdown: &watch::Receiver<Option<Instant>>,
    phase_deadline: Option<Instant>,
) -> bool {
    let signal_deadline = *shutdown.borrow();
    if shutdown.has_changed().is_err() && signal_deadline.is_none() {
        return false;
    }
    earliest(phase_deadline, signal_deadline).is_none_or(|value| Instant::now() < value)
}

/// Await one phase only until its local budget or retained stop cutoff.
#[cfg(test)]
pub(super) async fn bounded<T, F>(
    gate: &mut ShutdownGate<'_>,
    drain_timeout: Duration,
    phase_deadline: Option<Instant>,
    future: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    let mut future = Box::pin(future);
    loop {
        refresh(gate, drain_timeout, None);
        let deadline = earliest(phase_deadline, *gate.cutoff);
        if deadline.is_some_and(|value| Instant::now() >= value) {
            return None;
        }
        match deadline {
            Some(deadline) => {
                tokio::select! {
                    result = &mut future => return Some(result),
                    changed = gate.receiver.changed() => refresh(gate, drain_timeout, Some(changed)),
                    () = sleep_until(TokioInstant::from_std(deadline)) => return None,
                }
            }
            None => {
                tokio::select! {
                    result = &mut future => return Some(result),
                    changed = gate.receiver.changed() => refresh(gate, drain_timeout, Some(changed)),
                }
            }
        }
    }
}

/// Await a phase while durably fencing a signal or external drain request.
///
/// The operation is dropped at the retained absolute cutoff. While it runs,
/// the journal drain bit is checked periodically so a separate `drain` caller
/// cannot leave an in-flight pair stage outside the persisted fence.
pub(super) async fn bounded_persisting<T, F>(
    journal: &velnor_runner_journal::journal::Journal,
    gate: &mut ShutdownGate<'_>,
    drain_timeout: Duration,
    phase_deadline: Option<Instant>,
    future: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    bounded_persisting_inner(journal, gate, drain_timeout, phase_deadline, None, future).await
}

/// Bound a blocking protocol call and cancel it if a durable stop is observed
/// before the call's synchronous request dispatch begins.
pub(super) async fn bounded_protocol<T, F>(
    journal: &velnor_runner_journal::journal::Journal,
    gate: &mut ShutdownGate<'_>,
    drain_timeout: Duration,
    phase_deadline: Option<Instant>,
    dispatch: DispatchFence,
    future: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    bounded_persisting_inner(
        journal,
        gate,
        drain_timeout,
        phase_deadline,
        Some(dispatch),
        future,
    )
    .await
}

async fn bounded_persisting_inner<T, F>(
    journal: &velnor_runner_journal::journal::Journal,
    gate: &mut ShutdownGate<'_>,
    drain_timeout: Duration,
    phase_deadline: Option<Instant>,
    dispatch: Option<DispatchFence>,
    future: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    let mut future = Box::pin(future);
    let mut check_drain = Box::pin(sleep(DRAIN_CHECK_INTERVAL));
    loop {
        refresh(gate, drain_timeout, None);
        if dispatch_cancelled_by_cutoff(
            journal,
            gate,
            drain_timeout,
            phase_deadline,
            dispatch.as_ref(),
        )
        .await
        {
            return None;
        }
        let deadline = earliest(phase_deadline, *gate.cutoff);
        if deadline.is_some_and(|value| Instant::now() >= value) {
            if let Some(dispatch) = &dispatch {
                dispatch.cancel_pending();
            }
            return None;
        }
        let timeout = wait_until(deadline);
        tokio::pin!(timeout);
        tokio::select! {
            result = &mut future => {
                let _ = dispatch_cancelled_by_cutoff(
                    journal,
                    gate,
                    drain_timeout,
                    phase_deadline,
                    dispatch.as_ref(),
                ).await;
                return Some(result);
            }
            changed = gate.receiver.changed() => {
                refresh(gate, drain_timeout, Some(changed));
                if dispatch_cancelled_by_cutoff(
                    journal,
                    gate,
                    drain_timeout,
                    phase_deadline,
                    dispatch.as_ref(),
                ).await {
                    return None;
                }
            }
            () = &mut check_drain => {
                if dispatch_cancelled_by_cutoff(
                    journal,
                    gate,
                    drain_timeout,
                    phase_deadline,
                    dispatch.as_ref(),
                ).await {
                    return None;
                }
                check_drain.as_mut().reset(tokio::time::Instant::now() + DRAIN_CHECK_INTERVAL);
            }
            () = &mut timeout => {
                if let Some(dispatch) = &dispatch {
                    dispatch.cancel_pending();
                }
                return None;
            }
        }
    }
}

async fn wait_until(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        sleep_until(TokioInstant::from_std(deadline)).await;
    } else {
        std::future::pending::<()>().await;
    }
}

async fn dispatch_cancelled_by_cutoff(
    journal: &velnor_runner_journal::journal::Journal,
    gate: &mut ShutdownGate<'_>,
    drain_timeout: Duration,
    phase_deadline: Option<Instant>,
    dispatch: Option<&DispatchFence>,
) -> bool {
    let deadline = earliest(phase_deadline, *gate.cutoff);
    let observed = await_before_deadline(
        deadline,
        observe_cutoff_for(journal, gate.receiver, gate.cutoff, drain_timeout),
    )
    .await;
    let Some(observed) = observed else {
        if let Some(dispatch) = dispatch {
            dispatch.cancel_pending();
        }
        return true;
    };
    if !observed {
        return false;
    }
    let Some(dispatch) = dispatch else {
        return false;
    };
    dispatch.cancel_pending();
    dispatch.is_pending()
}

pub(in crate::linux) async fn observe_cutoff_before_deadline(
    journal: &velnor_runner_journal::journal::Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
    drain_timeout: Duration,
    phase_deadline: Option<Instant>,
) -> Option<bool> {
    let deadline = earliest(phase_deadline, *cutoff);
    await_before_deadline(
        deadline,
        observe_cutoff_for(journal, shutdown, cutoff, drain_timeout),
    )
    .await
}

async fn await_before_deadline<T, F>(deadline: Option<Instant>, future: F) -> Option<T>
where
    F: Future<Output = T>,
{
    let Some(deadline) = deadline else {
        return Some(future.await);
    };
    if Instant::now() >= deadline {
        return None;
    }
    tokio::select! {
        biased;
        () = sleep_until(TokioInstant::from_std(deadline)) => None,
        result = future => Some(result),
    }
}

fn earliest(left: Option<Instant>, right: Option<Instant>) -> Option<Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn refresh(
    gate: &mut ShutdownGate<'_>,
    drain_timeout: Duration,
    changed: Option<Result<(), watch::error::RecvError>>,
) {
    if gate.cutoff.is_some() {
        let _ = gate.receiver.borrow_and_update();
        return;
    }
    let latest = *gate.receiver.borrow_and_update();
    if latest.is_some() {
        *gate.cutoff = latest;
    } else if changed.is_some_and(|result| result.is_err()) || gate.receiver.has_changed().is_err()
    {
        *gate.cutoff = Some(
            Instant::now()
                .checked_add(drain_timeout)
                .unwrap_or_else(Instant::now),
        );
    }
}

#[cfg(test)]
#[path = "cutoff_tests.rs"]
mod tests;
