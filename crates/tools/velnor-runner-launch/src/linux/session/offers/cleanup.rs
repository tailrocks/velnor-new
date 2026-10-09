//! Existing bound-engine cleanup proof for correlated terminal generations.

use std::time::Instant;

use velnor_runner_host::worker::DiagnosticsStore;
use velnor_runner_journal::journal::Journal;

use super::super::{ShutdownGate, cutoff};
use crate::linux::LinuxLaunchContext;

pub(super) async fn cleanup_completed(
    context: &LinuxLaunchContext,
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    docker_binding: &velnor_runner_host::DockerDaemonBinding,
    shutdown: &mut ShutdownGate<'_>,
    completed: &[i64],
) -> bool {
    let local_deadline = Instant::now()
        .checked_add(context.drain_timeout())
        .unwrap_or_else(Instant::now);
    let deadline = shutdown
        .cutoff
        .map_or(local_deadline, |cutoff| cutoff.min(local_deadline));
    let cleanup = Box::pin(cutoff::bounded_persisting(
        journal,
        shutdown,
        context.drain_timeout(),
        Some(local_deadline),
        super::super::super::shutdown::cleanup_terminal_workers(
            journal,
            diagnostics,
            docker_binding,
            deadline,
        ),
    ))
    .await;
    if !matches!(cleanup, Some(Ok(_))) {
        return false;
    }
    let rows = cutoff::bounded_persisting(
        journal,
        shutdown,
        context.drain_timeout(),
        Some(local_deadline),
        journal.rows(),
    )
    .await;
    let Some(Ok(rows)) = rows else {
        return false;
    };
    completed.iter().all(|id| {
        rows.iter()
            .find(|row| row.id == *id)
            .is_some_and(|row| row.cleanup_proven)
    })
}
