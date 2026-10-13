//! Reconcile completed jobs before admitting work from the next poll.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use velnor_runner_github::{InnerJob, InnerKind, Transport};

use super::capacity;
use crate::journal::Journal;
use crate::listen::Secret;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::PairEngine;

const CLEANUP_CONCURRENCY_LIMIT: u32 = 4;
const CLEANUP_SCAN_LIMIT: u32 = 128;
const CLEANUP_LEASE_SECONDS: i64 = 120;

static ACTIVE_CLEANUPS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();

#[cfg(test)]
mod tests;

mod reconcile;
mod support;

use reconcile::{
    archive_store_for, cleanup_owned_pair, finish_proven_completion, observe_and_bind,
    open_archive_lease, remove_registered_runner,
};

use support::{completion_error, held, log_cleanup_error, map_journal, retry_at, unix_seconds};

pub(super) async fn record_completion_events(
    journal: &Journal,
    set_id: i64,
    polled: &velnor_runner_github::Poll,
) -> Result<(), EnsureError> {
    if let velnor_runner_github::Poll::Batch(batch) = polled {
        for job in &batch.jobs {
            if matches!(job.kind, InnerKind::Completed) {
                record_completion(journal, set_id, job).await?;
            }
        }
    }
    Ok(())
}

pub(super) async fn schedule_completed<T, E>(
    transport: T,
    set_id: i64,
    admin_token: &str,
    journal: Journal,
    docker: E,
) -> Result<Vec<tokio::task::JoinHandle<()>>, EnsureError>
where
    T: Transport + Clone + Send + 'static,
    E: PairEngine + Clone + Send + Sync + 'static,
{
    schedule_completed_with_slots(
        transport,
        set_id,
        admin_token,
        journal,
        docker,
        cleanup_slots(),
        usize::try_from(capacity::job_capacity().min(CLEANUP_CONCURRENCY_LIMIT))
            .unwrap_or(usize::MAX),
    )
    .await
}

#[cfg(test)]
pub(super) async fn schedule_completed_isolated<T, E>(
    transport: T,
    set_id: i64,
    admin_token: &str,
    journal: Journal,
    docker: E,
) -> Result<Vec<tokio::task::JoinHandle<()>>, EnsureError>
where
    T: Transport + Clone + Send + 'static,
    E: PairEngine + Clone + Send + Sync + 'static,
{
    schedule_completed_with_slots(
        transport,
        set_id,
        admin_token,
        journal,
        docker,
        Arc::new(AtomicUsize::new(0)),
        usize::try_from(CLEANUP_CONCURRENCY_LIMIT).unwrap_or(usize::MAX),
    )
    .await
}

async fn schedule_completed_with_slots<T, E>(
    transport: T,
    set_id: i64,
    admin_token: &str,
    journal: Journal,
    docker: E,
    active: Arc<AtomicUsize>,
    limit: usize,
) -> Result<Vec<tokio::task::JoinHandle<()>>, EnsureError>
where
    T: Transport + Clone + Send + 'static,
    E: PairEngine + Clone + Send + Sync + 'static,
{
    let now = unix_seconds()?;
    let pending = journal
        .due_completed_launches(now, CLEANUP_SCAN_LIMIT)
        .await
        .map_err(map_journal)?;
    let mut handles = Vec::new();
    let token = Secret::new(admin_token);
    let schedule = CleanupSchedule {
        transport: &transport,
        set_id,
        token: &token,
        journal,
        docker,
        active: &active,
        limit,
    };
    for row in pending {
        if let Some(handle) = schedule.one(row, now).await? {
            handles.push(handle);
        }
    }
    Ok(handles)
}

struct CleanupSchedule<'a, T, E> {
    transport: &'a T,
    set_id: i64,
    token: &'a Secret,
    journal: &'a Journal,
    docker: &'a E,
    active: &'a Arc<AtomicUsize>,
    limit: usize,
}

impl<T, E> CleanupSchedule<'_, T, E>
where
    T: Transport + Clone + Send + 'static,
    E: PairEngine + Clone + Send + Sync + 'static,
{
    async fn one(
        &self,
        row: IntentRow,
        now: i64,
    ) -> Result<Option<tokio::task::JoinHandle<()>>, EnsureError> {
        let lease_until = now
            .checked_add(CLEANUP_LEASE_SECONDS)
            .ok_or_else(completion_error)?;
        if !claim_cleanup_slot(self.active, self.limit) {
            return Ok(None);
        }
        let claim = self
            .journal
            .claim_completion_cleanup(row.id, now, lease_until)
            .await;
        let claim = match claim {
            Ok(Some(claim)) => claim,
            Ok(None) => {
                self.active.fetch_sub(1, Ordering::AcqRel);
                return Ok(None);
            }
            Err(error) => {
                self.active.fetch_sub(1, Ordering::AcqRel);
                return Err(map_journal(error));
            }
        };
        let task_journal = self.journal.clone();
        let task_docker = self.docker.clone();
        let task_token = Secret::new(self.token.expose());
        let task_row = row;
        let mut task_transport = self.transport.clone();
        let task_active = Arc::clone(self.active);
        let set_id = self.set_id;
        Ok(Some(tokio::task::spawn_blocking(move || {
            let _permit = CleanupPermit(task_active);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let Ok(runtime) = runtime else {
                return;
            };
            let result = runtime.block_on(reconcile_one(
                &mut task_transport,
                set_id,
                task_token.expose(),
                &task_journal,
                &task_docker,
                claim.generation,
                task_row.clone(),
            ));
            if !matches!(result, Ok(true)) {
                if let Ok(retry_at) = retry_at(claim.attempt) {
                    if let Err(error) = runtime.block_on(task_journal.retry_completion_cleanup(
                        task_row.id,
                        claim.generation,
                        retry_at,
                    )) {
                        log_cleanup_error(&task_row, "retry-journal", &error.to_string());
                    }
                }
            }
            if result.is_err() {
                eprintln!(
                    "completion_cleanup=hold launch_id={} attempt={} claim_generation={}",
                    task_row.launch_id.as_deref().unwrap_or("-"),
                    claim.attempt,
                    claim.generation
                );
            }
        })))
    }
}

async fn record_completion(
    journal: &Journal,
    set_id: i64,
    job: &InnerJob,
) -> Result<(), EnsureError> {
    let Some(request_id) = job.request_id.filter(|id| *id >= 0) else {
        return Err(completion_error());
    };
    let Some(runner_id) = job.runner_id.filter(|id| *id > 0) else {
        return Err(completion_error());
    };
    let Some(runner_name) = job.runner_name.as_deref() else {
        return Err(completion_error());
    };
    journal
        .record_runner_completed(set_id, request_id, runner_id, runner_name)
        .await
        .map_err(map_journal)?;
    Ok(())
}

async fn reconcile_one<T: Transport + ?Sized, E: PairEngine>(
    transport: &mut T,
    set_id: i64,
    admin_token: &str,
    journal: &Journal,
    docker: &E,
    claim_generation: i64,
    row: IntentRow,
) -> Result<bool, EnsureError> {
    let identity = journal.launch_identity(row.id).await.map_err(map_journal)?;
    let archive_store = match archive_store_for(journal, &row) {
        Ok(store) => store,
        Err(stage) => return held(&row, stage),
    };
    if journal
        .completion_worker_cleanup_proven(row.id)
        .await
        .map_err(map_journal)?
    {
        return finish_proven_completion(
            journal,
            &row,
            &identity,
            archive_store.as_ref(),
            claim_generation,
        )
        .await;
    }
    let archive_lease = match open_archive_lease(archive_store.as_ref(), &row, &identity) {
        Ok(lease) => lease,
        Err(stage) => return held(&row, stage),
    };
    let Some(observed) =
        observe_and_bind(docker, journal, &row, &identity, archive_lease.as_ref()).await?
    else {
        return Ok(false);
    };
    let runner_name = format!("v{}", identity.launch_id());
    if let Err(stage) = remove_registered_runner(
        transport,
        admin_token,
        set_id,
        &runner_name,
        row.github_runner_id.as_deref(),
    ) {
        return held(&row, stage);
    }
    if let Err(stage) =
        cleanup_owned_pair(docker, &identity, &observed, &row, archive_lease.as_ref()).await
    {
        return held(&row, stage);
    }
    let marked = journal
        .mark_completion_worker_cleanup_proven(row.id, claim_generation, unix_seconds()?)
        .await
        .map_err(map_journal)?;
    if !marked {
        return held(&row, "cleanup-claim-expired");
    }
    finish_proven_completion(
        journal,
        &row,
        &identity,
        archive_store.as_ref(),
        claim_generation,
    )
    .await
}

fn cleanup_slots() -> Arc<AtomicUsize> {
    Arc::clone(ACTIVE_CLEANUPS.get_or_init(|| Arc::new(AtomicUsize::new(0))))
}

fn claim_cleanup_slot(active_count: &AtomicUsize, limit: usize) -> bool {
    let mut active = active_count.load(Ordering::Acquire);
    loop {
        if active >= limit {
            return false;
        }
        match active_count.compare_exchange_weak(
            active,
            active.saturating_add(1),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(current) => active = current,
        }
    }
}

struct CleanupPermit(Arc<AtomicUsize>);

impl Drop for CleanupPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
