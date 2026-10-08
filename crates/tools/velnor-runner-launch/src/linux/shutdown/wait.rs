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
                match journal.draining().await {
                    Ok(true) | Err(_) => return deadline_after(Instant::now(), drain_timeout),
                    Ok(false) => {}
                }
                wake.as_mut().reset(tokio::time::Instant::now() + SIGNAL_POLL);
            }
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
