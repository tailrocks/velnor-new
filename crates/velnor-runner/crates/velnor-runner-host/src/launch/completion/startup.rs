// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use tokio::runtime::Runtime;

use crate::error::HostError;

pub(super) fn spawn_with_runtime(
    build: fn() -> std::io::Result<Runtime>,
    body: impl FnOnce(&Runtime) + Send + 'static,
) -> Result<JoinHandle<()>, HostError> {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("velnor-completion-reconciler".to_owned())
        .spawn(move || match build() {
            Ok(runtime) => {
                if ready_tx.send(true).is_ok() {
                    body(&runtime);
                }
            }
            Err(_) => {
                ready_tx.send(false).ok();
            }
        })
        .map_err(|_| HostError::Journal)?;
    if ready_rx.recv() == Ok(true) {
        return Ok(thread);
    }
    drop(thread.join());
    Err(HostError::Journal)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn failing() -> std::io::Result<Runtime> {
        Err(std::io::Error::other("no io driver"))
    }

    fn current_thread() -> std::io::Result<Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    }

    #[test]
    fn a_runtime_that_cannot_be_built_fails_the_start_and_never_runs_the_body() {
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let started = spawn_with_runtime(failing, move |_| flag.store(true, Ordering::SeqCst));
        assert!(matches!(started, Err(HostError::Journal)));
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[test]
    fn a_built_runtime_runs_the_body_on_the_started_thread() {
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let thread =
            spawn_with_runtime(current_thread, move |_| flag.store(true, Ordering::SeqCst))
                .unwrap_or_else(|_| unreachable!());
        thread.join().unwrap_or_else(|_| unreachable!());
        assert!(ran.load(Ordering::SeqCst));
    }
}
