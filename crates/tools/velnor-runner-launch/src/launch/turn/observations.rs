//! Durable observations from each actual queue poll.

use velnor_runner_github::{
    ActionsJobReconciliationState, AsyncDiscoveryTransport, ObservedScaleSetJob, Poll,
    reconcile_observed_scale_set_job_async,
};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_journal::journal::{IntentState, Journal, LaunchEffectState, RunnerStartIntent};
use velnor_runner_journal::reconcile::IntentRow;

/// Persist actual lifecycle identities before admission decisions.
pub(super) async fn persist_lifecycle_events(
    journal: &Journal,
    polled: &Poll,
) -> Result<(), EnsureError> {
    if let Poll::Batch(batch) = polled {
        for event in &batch.jobs {
            journal
                .observe_runner_event(event)
                .await
                .map_err(map_journal)?;
        }
    }

    Ok(())
}

/// Whether any persisted actual runner identity still needs Actions evidence.
pub(super) async fn has_pending_actions_reconciliation(
    journal: &Journal,
) -> Result<bool, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    Ok(rows.iter().any(needs_actions_reconciliation))
}

/// Reconcile exact persisted lifecycle identities within one bounded read cycle.
///
/// This runs in the owner of the current Scale Set session. The REST helper is
/// GET-only. Incomplete results leave the launch row occupied; a completed
/// result records remote evidence but is not local cleanup proof.
pub(super) async fn reconcile_pending_lifecycle(
    journal: &Journal,
    transport: &mut (impl AsyncDiscoveryTransport + ?Sized),
    owner: &str,
    repository: &str,
    actions_token: &str,
    deadline: tokio::time::Instant,
) -> Result<bool, EnsureError> {
    // Re-read only actual persisted lifecycle identities. This also retries
    // read-only reconciliation on an Empty poll while this same session stays
    // open; Available/request fields never become a runner identity.
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut pending = false;
    for row in rows {
        if !needs_actions_reconciliation(&row) {
            continue;
        }
        if tokio::time::Instant::now() >= deadline {
            pending = true;
            break;
        }
        let Some(observed) = observed_scale_set_job(&row) else {
            pending = true;
            continue;
        };
        let Ok(Ok(reconciliation)) = tokio::time::timeout_at(
            deadline,
            reconcile_observed_scale_set_job_async(
                transport,
                owner,
                repository,
                observed,
                actions_token,
            ),
        )
        .await
        else {
            // A read-only timeout or REST error is unresolved evidence. Keep
            // the durable lifecycle identity and continue the same session.
            pending = true;
            break;
        };
        if reconciliation.state == ActionsJobReconciliationState::Completed {
            journal
                .record_actions_job_reconciliation(row.id, &reconciliation)
                .await
                .map_err(map_journal)?;
        } else {
            pending = true;
        }
    }
    Ok(pending)
}

/// Test/helper boundary for a single directly requested reconciliation cycle.
#[cfg(test)]
pub(super) async fn persist_observed_lifecycle(
    journal: &Journal,
    polled: &Poll,
    transport: &mut (impl AsyncDiscoveryTransport + ?Sized),
    owner: &str,
    repository: &str,
    actions_token: &str,
    deadline: tokio::time::Instant,
) -> Result<bool, EnsureError> {
    persist_lifecycle_events(journal, polled).await?;
    reconcile_pending_lifecycle(
        journal,
        transport,
        owner,
        repository,
        actions_token,
        deadline,
    )
    .await
}

fn needs_actions_reconciliation(row: &IntentRow) -> bool {
    row.kind == "launch"
        && matches!(row.state, IntentState::Done | IntentState::Uncertain)
        && row.launch_effect == LaunchEffectState::MayHaveEffect
        && row.docker_id.is_some()
        && row.runner_start_intent == RunnerStartIntent::MayHaveStarted
        && row.observed_job_id.is_some()
        && row.observed_workflow_run_id.is_some()
        && row.github_runner_id.is_some()
        && row.runner_name.is_some()
        && row.observed_actions_attempt.is_none()
        && row.observed_actions_job_id.is_none()
        && !row.remote_terminal
        && !row.cleanup_proven
}

fn observed_scale_set_job(row: &IntentRow) -> Option<ObservedScaleSetJob<'_>> {
    Some(ObservedScaleSetJob {
        scale_set_job_id: Some(row.observed_job_id.as_deref()?),
        workflow_run_id: Some(row.observed_workflow_run_id?),
        runner_id: Some(row.github_runner_id.as_deref()?.parse().ok()?),
        runner_name: Some(row.runner_name.as_deref()?),
    })
}

fn map_journal(error: velnor_runner_host::HostError) -> EnsureError {
    match error {
        velnor_runner_host::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
