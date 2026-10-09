//! Bounded recovery of unbound Linux launches with exact durable lifecycle evidence.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, timeout_at};
use velnor_runner_github::DiscoveryTransport;
use velnor_runner_github::{
    ActionsJobReconciliation, ActionsJobReconciliationState, ObservedScaleSetJob,
    reconcile_observed_scale_set_job_async,
};
use velnor_runner_host::BoundedDiscoveryTransport;
use velnor_runner_host::worker::{OwnedDockerResource, list_owned_docker_resources_bound_until};
use velnor_runner_journal::journal::{Journal, JournalDockerDaemonBinding, LegacyLaunchAdoption};
use velnor_runner_journal::reconcile::IntentRow;
use velnor_runner_launch_slot::holds;

use super::super::cutoff;
use super::super::{CancelDispatchOnDrop, DeadlineBoundTransport, DispatchFence, ShutdownGate};
use super::{BatchWork, inventory};

const RECOVERY_WINDOW: Duration = Duration::from_secs(120);
const RECOVERY_PHASE: Duration = Duration::from_secs(20);
const MAX_RECOVERY_ATTEMPTS: usize = 8;
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

type RecoveryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One bounded in-memory retry wave. Durable evidence remains in the Journal;
/// exhausting this budget leaves unresolved generations occupied for a later
/// process/session reconciliation.
#[derive(Default)]
pub(in crate::linux::session) struct RecoveryBudget {
    started_at: Option<Instant>,
    attempts: usize,
    retry_at: Option<Instant>,
}

impl RecoveryBudget {
    fn next_deadline(&mut self, now: Instant, stop_at: Option<Instant>) -> Option<Instant> {
        let started_at = *self.started_at.get_or_insert(now);
        let window_end = started_at.checked_add(RECOVERY_WINDOW)?;
        if now >= window_end
            || self.attempts >= MAX_RECOVERY_ATTEMPTS
            || self.retry_at.is_some_and(|retry_at| now < retry_at)
        {
            return None;
        }
        let phase_end = now.checked_add(RECOVERY_PHASE)?.min(window_end);
        let phase_end = stop_at.map_or(phase_end, |stop_at| phase_end.min(stop_at));
        if now >= phase_end {
            return None;
        }
        let delay = RETRY_DELAYS[self.attempts.min(RETRY_DELAYS.len() - 1)];
        self.attempts += 1;
        self.retry_at = now.checked_add(delay);
        Some(phase_end)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

trait RecoveryServices {
    fn completed_rest(
        &mut self,
        row: IntentRow,
        deadline: TokioInstant,
    ) -> RecoveryFuture<'_, Option<ActionsJobReconciliation>>;

    fn complete_inventory(
        &mut self,
        binding: JournalDockerDaemonBinding,
        deadline: TokioInstant,
    ) -> RecoveryFuture<'_, Option<Vec<OwnedDockerResource>>>;
}

struct LinuxRecoveryServices<'a> {
    repository: &'a str,
    actions_read: &'a str,
    docker_binding: &'a velnor_runner_host::DockerDaemonBinding,
    shutdown: watch::Receiver<Option<Instant>>,
}

impl RecoveryServices for LinuxRecoveryServices<'_> {
    fn completed_rest(
        &mut self,
        row: IntentRow,
        deadline: TokioInstant,
    ) -> RecoveryFuture<'_, Option<ActionsJobReconciliation>> {
        Box::pin(async move {
            let (owner, repository) = self.repository.split_once('/')?;
            if owner.is_empty() || repository.is_empty() {
                return None;
            }
            let observed = ObservedScaleSetJob {
                scale_set_job_id: row.observed_job_id.as_deref(),
                workflow_run_id: row.observed_workflow_run_id,
                runner_id: row
                    .github_runner_id
                    .as_deref()
                    .and_then(|id| id.parse::<i64>().ok()),
                runner_name: row.runner_name.as_deref(),
            };
            let dispatch = DispatchFence::new();
            if !dispatch.begin(&self.shutdown, Some(deadline.into_std())) {
                return None;
            }
            let _cancel_if_dropped = CancelDispatchOnDrop(dispatch.clone());
            let mut transport = DeadlineBoundTransport::new(
                BoundedDiscoveryTransport::new(),
                dispatch,
                self.shutdown.clone(),
                Some(deadline.into_std()),
            );
            transport.bind_github_api_origin().ok()?;
            timeout_at(
                deadline,
                reconcile_observed_scale_set_job_async(
                    &mut transport,
                    owner,
                    repository,
                    observed,
                    self.actions_read,
                ),
            )
            .await
            .ok()?
            .ok()
        })
    }

    fn complete_inventory(
        &mut self,
        binding: JournalDockerDaemonBinding,
        deadline: TokioInstant,
    ) -> RecoveryFuture<'_, Option<Vec<OwnedDockerResource>>> {
        Box::pin(async move {
            let observed = JournalDockerDaemonBinding::new(
                self.docker_binding.endpoint(),
                self.docker_binding.engine_id(),
            )
            .ok()?;
            if observed != binding {
                return None;
            }
            timeout_at(
                deadline,
                list_owned_docker_resources_bound_until(self.docker_binding, deadline),
            )
            .await
            .ok()?
            .ok()
        })
    }
}

struct RecoveryPass {
    completed: Vec<i64>,
    had_candidates: bool,
}

pub(super) async fn recover_pending(work: &mut BatchWork<'_>) -> Option<Vec<i64>> {
    let repository = &work.active.binding.repository_full_name;
    if repository
        .split_once('/')
        .is_none_or(|(owner, name)| owner.is_empty() || name.is_empty())
    {
        return Some(Vec::new());
    }
    let mut services = LinuxRecoveryServices {
        repository,
        actions_read: &work.credentials.actions_read,
        docker_binding: &work.active.docker_binding,
        shutdown: work.shutdown.receiver.clone(),
    };
    run_recovery_pass(
        work.journal,
        &work.active.journal_binding,
        &mut services,
        work.recovery_budget,
        work.context.drain_timeout(),
        &mut work.shutdown,
    )
    .await
}

async fn run_recovery_pass<S: RecoveryServices>(
    journal: &Journal,
    active_binding: &JournalDockerDaemonBinding,
    services: &mut S,
    budget: &mut RecoveryBudget,
    drain_timeout: Duration,
    shutdown: &mut ShutdownGate<'_>,
) -> Option<Vec<i64>> {
    let Some(phase_deadline) = budget.next_deadline(Instant::now(), *shutdown.cutoff) else {
        return Some(Vec::new());
    };
    let pass = cutoff::bounded_persisting(
        journal,
        shutdown,
        drain_timeout,
        Some(phase_deadline),
        reconcile_and_adopt(journal, active_binding, services, phase_deadline),
    )
    .await?;
    let pass = pass?;
    if !pass.had_candidates {
        budget.reset();
    }
    Some(pass.completed)
}

async fn reconcile_and_adopt<S: RecoveryServices>(
    journal: &Journal,
    active_binding: &JournalDockerDaemonBinding,
    services: &mut S,
    deadline: Instant,
) -> Option<RecoveryPass> {
    let mut candidates = journal.unbound_started_launches().await.ok()?;
    if candidates.is_empty() {
        return Some(RecoveryPass {
            completed: Vec::new(),
            had_candidates: false,
        });
    }
    let tokio_deadline = TokioInstant::from_std(deadline);
    let mut completed = Vec::new();
    while let Some(candidate) = candidates.first().cloned() {
        candidates.remove(0);
        if Instant::now() >= deadline {
            break;
        }
        let current = if has_completed_rest_receipt(&candidate) {
            candidate
        } else {
            let Some(rest) = services
                .completed_rest(candidate.clone(), tokio_deadline)
                .await
            else {
                continue;
            };
            if rest.state != ActionsJobReconciliationState::Completed {
                continue;
            }
            if journal
                .record_actions_job_reconciliation(candidate.id, &rest)
                .await
                .is_err()
            {
                return None;
            }
            journal
                .rows()
                .await
                .ok()?
                .into_iter()
                .find(|row| row.id == candidate.id)?
        };

        let rows = journal.rows().await.ok()?;
        if !all_existing_bindings_match(journal, &rows, active_binding, tokio_deadline).await {
            continue;
        }
        let Some(resources) = services
            .complete_inventory(active_binding.clone(), tokio_deadline)
            .await
        else {
            continue;
        };
        if Instant::now() >= deadline || !inventory::inventory_matches_rows(&resources, &rows) {
            continue;
        }
        if !rows.iter().any(|row| row == &current) {
            continue;
        }
        match journal
            .adopt_legacy_launch_on_engine(&current, active_binding)
            .await
        {
            Ok(LegacyLaunchAdoption::Adopted | LegacyLaunchAdoption::AlreadyAdopted) => {
                completed.push(current.id);
            }
            Err(_) => {}
        }
    }
    Some(RecoveryPass {
        completed,
        had_candidates: true,
    })
}

fn has_completed_rest_receipt(row: &IntentRow) -> bool {
    row.remote_terminal
        && row.observed_actions_attempt.is_some()
        && row.observed_actions_job_id.is_some()
}

async fn all_existing_bindings_match(
    journal: &Journal,
    rows: &[IntentRow],
    active_binding: &JournalDockerDaemonBinding,
    deadline: TokioInstant,
) -> bool {
    for row in rows.iter().filter(|row| row.kind == "launch" && holds(row)) {
        let Ok(Ok(binding)) = timeout_at(deadline, journal.launch_daemon_binding(row.id)).await
        else {
            return false;
        };
        if binding.is_some_and(|binding| binding != *active_binding) {
            return false;
        }
    }
    TokioInstant::now() < deadline
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
