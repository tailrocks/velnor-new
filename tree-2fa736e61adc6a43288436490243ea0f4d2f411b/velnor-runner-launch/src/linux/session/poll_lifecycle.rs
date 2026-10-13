//! Empty-poll recovery and delivered-batch progression for a Linux session.

use std::future::Future;
use std::time::Instant;

use tokio::sync::watch;
use velnor_runner_github::policy::ParsedTrustBatch;
use velnor_runner_host::worker::DiagnosticsStore;
use velnor_runner_journal::journal::Journal;

use super::poll_retry::wait_before_next_poll;
use super::{ActiveSession, ShutdownGate, close_after_cutoff, offers};
use crate::linux::{LinuxLaunchContext, LinuxLaunchCredentials, SIGNAL_POLL};

pub(super) struct RecoveryPollWork<'a> {
    pub(super) context: &'a LinuxLaunchContext,
    pub(super) journal: &'a Journal,
    pub(super) diagnostics: &'a DiagnosticsStore,
    pub(super) docker: &'a bollard::Docker,
    pub(super) active: &'a mut ActiveSession,
    pub(super) credentials: &'a LinuxLaunchCredentials,
    pub(super) recovery_budget: &'a mut offers::RecoveryBudget,
    pub(super) shutdown: &'a mut watch::Receiver<Option<Instant>>,
    pub(super) cutoff: &'a mut Option<Instant>,
}

pub(super) async fn process_empty_poll(work: RecoveryPollWork<'_>) -> bool {
    if !offers::recover_after_empty(offers::BatchWork {
        context: work.context,
        journal: work.journal,
        diagnostics: work.diagnostics,
        docker: work.docker,
        active: &*work.active,
        credentials: work.credentials,
        recovery_budget: &mut *work.recovery_budget,
        shutdown: ShutdownGate {
            receiver: &mut *work.shutdown,
            cutoff: &mut *work.cutoff,
        },
    })
    .await
    {
        return false;
    }
    if work.cutoff.is_some()
        && close_after_cutoff(
            work.journal,
            work.diagnostics,
            work.active,
            work.cutoff.as_ref(),
        )
        .await
    {
        return false;
    }
    let _ = wait_before_next_poll(
        work.journal,
        work.shutdown,
        work.cutoff,
        work.context.drain_timeout(),
        SIGNAL_POLL,
    )
    .await;
    true
}

pub(super) async fn process_delivered_batch(
    work: RecoveryPollWork<'_>,
    batch: ParsedTrustBatch,
) -> offers::BatchOutcome {
    let outcome = Box::pin(offers::process_batch(
        offers::BatchWork {
            context: work.context,
            journal: work.journal,
            diagnostics: work.diagnostics,
            docker: work.docker,
            active: &*work.active,
            credentials: work.credentials,
            recovery_budget: &mut *work.recovery_budget,
            shutdown: ShutdownGate {
                receiver: &mut *work.shutdown,
                cutoff: &mut *work.cutoff,
            },
        },
        batch,
    ))
    .await;
    finish_delivered_batch(outcome, work.cutoff.is_some(), || async {
        close_after_cutoff(
            work.journal,
            work.diagnostics,
            work.active,
            work.cutoff.as_ref(),
        )
        .await
    })
    .await
}

/// Apply the production post-message close rule after its processing result.
/// Held or stopped outcomes never reach a session close operation.
pub(super) async fn finish_delivered_batch<C, Fut>(
    outcome: offers::BatchOutcome,
    cutoff_pending: bool,
    close: C,
) -> offers::BatchOutcome
where
    C: FnOnce() -> Fut,
    Fut: Future<Output = bool>,
{
    match outcome {
        offers::BatchOutcome::AvailableOffersHeld | offers::BatchOutcome::Stopped => {
            return outcome;
        }
        offers::BatchOutcome::Advanced => {}
    }
    if cutoff_pending && close().await {
        offers::BatchOutcome::Stopped
    } else {
        offers::BatchOutcome::Advanced
    }
}
