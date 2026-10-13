//! Linux SIGTERM/SIGINT adapter for a single absolute daemon drain cutoff.
//!
//! The daemon coordinator consumes the watch receiver. This module deliberately
//! does not run, stop, or report success for the coordinator itself.

use std::io;
use std::time::{Duration, Instant};

use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Keeps the signal listener alive while the Linux daemon coordinator runs.
#[must_use]
pub(crate) struct ShutdownSignalAdapter {
    receiver: watch::Receiver<Option<Instant>>,
    task: JoinHandle<()>,
}

impl ShutdownSignalAdapter {
    /// Clone the receiver consumed by the state-owned daemon coordinator.
    #[must_use]
    pub(crate) fn receiver(&self) -> watch::Receiver<Option<Instant>> {
        self.receiver.clone()
    }
}

impl Drop for ShutdownSignalAdapter {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Register both Linux shutdown signals and publish one monotonic cutoff on the
/// first signal. Call from inside the CLI-owned Tokio runtime before starting
/// the daemon coordinator.
pub(crate) fn install(timeout: Duration) -> io::Result<ShutdownSignalAdapter> {
    let terminate = signal(SignalKind::terminate())?;
    let interrupt = signal(SignalKind::interrupt())?;
    let (sender, receiver) = watch::channel(None);
    let task = tokio::spawn(wait_for_first_signal(terminate, interrupt, sender, timeout));
    Ok(ShutdownSignalAdapter { receiver, task })
}

async fn wait_for_first_signal(
    mut terminate: Signal,
    mut interrupt: Signal,
    sender: watch::Sender<Option<Instant>>,
    timeout: Duration,
) {
    tokio::select! {
        _ = terminate.recv() => {},
        _ = interrupt.recv() => {},
    }
    let observed_at = Instant::now();
    publish_first_deadline(&sender, timeout, observed_at);
}

fn publish_first_deadline(
    sender: &watch::Sender<Option<Instant>>,
    timeout: Duration,
    observed_at: Instant,
) -> Instant {
    let proposed = observed_at.checked_add(timeout).unwrap_or(observed_at);
    let mut selected = proposed;
    let _changed = sender.send_if_modified(|published| {
        if let Some(existing) = *published {
            selected = existing;
            false
        } else {
            *published = Some(proposed);
            true
        }
    });
    selected
}

#[cfg(test)]
mod tests {
    use super::publish_first_deadline;
    use std::time::{Duration, Instant};
    use tokio::sync::watch;

    #[test]
    fn later_signal_cannot_extend_first_deadline() {
        let (sender, receiver) = watch::channel(None);
        let first_signal = Instant::now();
        let expected = first_signal + Duration::from_secs(30);

        let first = publish_first_deadline(&sender, Duration::from_secs(30), first_signal);
        let second = publish_first_deadline(
            &sender,
            Duration::from_secs(30),
            first_signal + Duration::from_secs(20),
        );

        assert_eq!(first, expected);
        assert_eq!(second, expected);
        assert_eq!(*receiver.borrow(), Some(expected));
    }

    #[test]
    fn overflowing_deadline_expires_at_signal_observation() {
        let (sender, receiver) = watch::channel(None);
        let observed_at = Instant::now();

        let deadline = publish_first_deadline(&sender, Duration::MAX, observed_at);

        assert_eq!(deadline, observed_at);
        assert_eq!(*receiver.borrow(), Some(observed_at));
    }
}
