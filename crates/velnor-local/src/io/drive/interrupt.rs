// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

/// Whether the user or a supervisor told `run` to stop: SIGINT (Ctrl-C) or SIGTERM (`timeout`,
/// `kill`, a cancelled CI job). Both handlers are installed before any container exists, so no
/// signal kills the process while an environment is up.
#[derive(Debug)]
pub(super) struct Interrupt(watch::Receiver<bool>);

impl Interrupt {
    /// The caller learns of the signal through `raised`; `first` runs once, on the first signal.
    pub(super) fn watch(first: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        let (raise, raised) = watch::channel(false);
        tokio::spawn(async move {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
            first();
            let _ = raise.send(true);
            // Later signals are consumed: the teardown the first one started is not cut short.
            loop {
                tokio::select! {
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => {}
                }
            }
        });
        Ok(Self(raised))
    }

    pub(super) async fn raised(&self) {
        let mut raised = self.0.clone();
        let _ = raised.wait_for(|raised| *raised).await;
    }
}
