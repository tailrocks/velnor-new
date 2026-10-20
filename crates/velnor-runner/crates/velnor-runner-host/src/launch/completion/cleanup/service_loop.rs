//! Run bounded cleanup scans until shutdown.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::error::HostError;

use super::{Context, Failure, reconcile_due_burst};
use crate::launch::completion::Resources;

const SCAN_INTERVAL: Duration = Duration::from_secs(30);

pub(in crate::launch::completion) fn run(
    runtime: &tokio::runtime::Runtime,
    resources: Resources,
    stopping: &Arc<AtomicBool>,
    receiver: &Receiver<()>,
) -> Result<(), HostError> {
    let Resources {
        journal,
        docker,
        mut transport,
        admin,
        diagnostics,
        native_cleanup,
        selected_engine,
    } = resources;
    let mut context = Context {
        journal: &journal,
        docker: &docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: native_cleanup.as_ref(),
        selected_engine: selected_engine.as_ref(),
    };
    let mut first_failure = None;
    loop {
        if stopping.load(Ordering::Acquire) {
            return first_failure.map_or(Ok(()), Err);
        }
        match runtime.block_on(reconcile_due_burst(&mut context, stopping)) {
            Ok((_, failure)) => remember_failure(&mut first_failure, failure),
            Err(failure) => {
                failure.report();
                remember_failure(&mut first_failure, Some(failure));
            }
        }
        if stopping.load(Ordering::Acquire) {
            return first_failure.map_or(Ok(()), Err);
        }
        match receiver.recv_timeout(SCAN_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return first_failure.map_or(Ok(()), Err);
            }
        }
    }
}

fn remember_failure(first: &mut Option<HostError>, failure: Option<Failure>) {
    if first.is_none()
        && let Some(failure) = failure
    {
        *first = Some(failure.error);
    }
}
