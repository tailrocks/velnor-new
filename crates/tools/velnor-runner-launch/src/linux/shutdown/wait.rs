use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, sleep, timeout_at};

use velnor_runner_journal::journal::Journal;

use super::super::{SIGNAL_POLL, deadline_after};

pub(crate) async fn wait_for_shutdown(
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    drain_timeout: Duration,
) -> Instant {
    let mut wake = Box::pin(sleep(SIGNAL_POLL));
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                let latest = *shutdown.borrow_and_update();
                if let Some(deadline) = retained_cutoff(
                    latest,
                    changed.is_err(),
                    drain_timeout,
                    Instant::now(),
                ) {
                    return deadline;
                }
            }
            () = &mut wake => {
                let observed_at = Instant::now();
                let probe_deadline = deadline_after(observed_at, Duration::from_millis(250));
                let drain = tokio::select! {
                    result = timeout_at(TokioInstant::from_std(probe_deadline), journal.draining()) => result,
                    changed = shutdown.changed() => {
                        let latest = *shutdown.borrow_and_update();
                        return retained_cutoff(
                            latest,
                            changed.is_err(),
                            drain_timeout,
                            Instant::now(),
                        ).unwrap_or_else(|| deadline_after(Instant::now(), drain_timeout));
                    }
                };
                match drain {
                    Ok(Ok(true) | Err(_)) | Err(_) => {
                        return deadline_after(Instant::now(), drain_timeout);
                    }
                    Ok(Ok(false)) => {}
                }
                wake.as_mut().reset(tokio::time::Instant::now() + SIGNAL_POLL);
            }
        }
    }
}

pub(crate) async fn read_startup_drain(
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
    drain_timeout: Duration,
) -> Option<bool> {
    if cutoff.is_some() {
        return None;
    }
    let deadline = deadline_after(Instant::now(), drain_timeout);
    tokio::select! {
        result = timeout_at(TokioInstant::from_std(deadline), journal.draining()) => {
            match result {
                Ok(Ok(value)) => Some(value),
                Ok(Err(_)) | Err(_) => None,
            }
        }
        changed = shutdown.changed() => {
            let latest = *shutdown.borrow_and_update();
            *cutoff = Some(
                retained_cutoff(latest, changed.is_err(), drain_timeout, Instant::now())
                    .unwrap_or_else(|| deadline_after(Instant::now(), drain_timeout)),
            );
            None
        }
    }
}

pub(crate) fn retained_cutoff(
    latest: Option<Instant>,
    channel_closed: bool,
    drain_timeout: Duration,
    observed_at: Instant,
) -> Option<Instant> {
    latest.or_else(|| channel_closed.then(|| deadline_after(observed_at, drain_timeout)))
}

pub(crate) async fn confirm_drain(journal: &Journal, deadline: Instant) -> bool {
    if Instant::now() >= deadline {
        return false;
    }
    let tokio_deadline = TokioInstant::from_std(deadline);
    if !matches!(
        timeout_at(tokio_deadline, journal.request_drain()).await,
        Ok(Ok(()))
    ) {
        return false;
    }
    matches!(
        timeout_at(tokio_deadline, journal.draining()).await,
        Ok(Ok(true))
    )
}
