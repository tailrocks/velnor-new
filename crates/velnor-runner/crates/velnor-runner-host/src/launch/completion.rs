//! Reconcile completed jobs before admitting work from the next poll.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use velnor_runner_github::{InnerJob, InnerKind, Transport, get_runner_by_name, remove_runner};

use super::capacity;
use crate::journal::Journal;
use crate::listen::Secret;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::{PairEngine, cleanup_worker, reconcile_worker};

const CLEANUP_CONCURRENCY_LIMIT: u32 = 4;
const CLEANUP_SCAN_LIMIT: u32 = 128;
const CLEANUP_LEASE_SECONDS: i64 = 120;

static ACTIVE_CLEANUPS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) use tests::{BlockingRunnerApi, CompletionEngine};

mod support;

use support::{
    completion_error, held, log_cleanup_error, map_journal, open_archive_store, retry_at,
    runner_matches, unix_seconds,
};

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
    let lease_until = now
        .checked_add(CLEANUP_LEASE_SECONDS)
        .ok_or_else(completion_error)?;
    for row in pending {
        if !claim_cleanup_slot(&active, limit) {
            continue;
        }
        let claim = journal
            .claim_completion_cleanup(row.id, now, lease_until)
            .await;
        let claim = match claim {
            Ok(Some(claim)) => claim,
            Ok(None) => {
                active.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            Err(error) => {
                active.fetch_sub(1, Ordering::AcqRel);
                return Err(map_journal(error));
            }
        };
        let task_journal = journal.clone();
        let task_docker = docker.clone();
        let task_token = Secret::new(token.expose());
        let task_row = row.clone();
        let mut task_transport = transport.clone();
        let task_active = Arc::clone(&active);
        handles.push(tokio::task::spawn_blocking(move || {
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
        }));
    }
    drop(transport);
    Ok(handles)
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
    row: IntentRow,
) -> Result<bool, EnsureError> {
    let identity = journal.launch_identity(row.id).await.map_err(map_journal)?;
    let archive_store = if row.seed_generation_id.is_some() {
        match open_archive_store(journal) {
            Ok(store) => Some(store),
            Err(_) => return held(&row, "archive-store-unavailable"),
        }
    } else {
        None
    };
    if journal
        .completion_worker_cleanup_proven(row.id)
        .await
        .map_err(map_journal)?
    {
        if let (Some(store), Some(generation)) =
            (archive_store.as_ref(), row.seed_generation_id.as_deref())
        {
            if store
                .release_after_confirmed_cleanup(identity.launch_id(), generation)
                .is_err()
            {
                return held(&row, "archive-lease-retirement");
            }
        }
        journal.record_cleanup(row.id).await.map_err(map_journal)?;
        return Ok(true);
    }
    let archive_lease = match (archive_store.as_ref(), row.seed_generation_id.as_deref()) {
        (Some(store), Some(generation)) => {
            match store.open_existing_lease(identity.launch_id(), generation) {
                Ok(lease) => Some(lease),
                Err(_) => return held(&row, "archive-lease-unavailable"),
            }
        }
        (None, None) => None,
        _ => return held(&row, "archive-lease-identity"),
    };
    let runner_name = format!("v{}", identity.launch_id());
    let observed = match reconcile_worker(
        docker,
        &identity,
        row.docker_id.as_deref(),
        row.dind_id.as_deref(),
        archive_lease.as_ref(),
    )
    .await
    {
        Ok(observed) => observed,
        Err(_) => return held(&row, "docker-observation"),
    };
    if observed.runner_running() == Some(true) {
        return held(&row, "runner-active");
    }
    if observed.runner_id().is_some() && observed.runner_running() != Some(false) {
        return held(&row, "runner-state-unknown");
    }
    if let Some(runner_id) = observed.runner_id() {
        journal
            .bind_runner_container(row.id, runner_id)
            .await
            .map_err(map_journal)?;
    }
    if let Some(dind_id) = observed.dind_id() {
        journal
            .bind_dind_container(row.id, dind_id)
            .await
            .map_err(map_journal)?;
    }
    let github_runner_id = row
        .github_runner_id
        .as_deref()
        .and_then(|id| id.parse::<i64>().ok());
    let Some(github_runner_id) = github_runner_id else {
        return held(&row, "github-id-missing");
    };
    match get_runner_by_name(transport, &runner_name, admin_token) {
        Ok(Some(runner)) if runner_matches(&runner, github_runner_id, set_id, &runner_name) => {
            if remove_runner(transport, runner.id, admin_token).is_err() {
                return held(&row, "github-delete");
            }
            match get_runner_by_name(transport, &runner_name, admin_token) {
                Ok(None) => {}
                Ok(Some(_)) => return held(&row, "github-still-present"),
                Err(_) => return held(&row, "github-confirm-absence"),
            }
        }
        Ok(None) => {}
        Ok(Some(_)) => return held(&row, "github-identity"),
        Err(_) => return held(&row, "github-lookup"),
    }
    if cleanup_worker(
        docker,
        &identity,
        observed.runner_id().or(row.docker_id.as_deref()),
        observed.dind_id().or(row.dind_id.as_deref()),
        archive_lease.as_ref(),
    )
    .await
    .is_err()
    {
        return held(&row, "docker-cleanup");
    }
    journal
        .mark_completion_worker_cleanup_proven(row.id)
        .await
        .map_err(map_journal)?;
    if let (Some(store), Some(generation)) =
        (archive_store.as_ref(), row.seed_generation_id.as_deref())
    {
        if store
            .release_after_confirmed_cleanup(identity.launch_id(), generation)
            .is_err()
        {
            return held(&row, "archive-lease-retirement");
        }
    }
    journal.record_cleanup(row.id).await.map_err(map_journal)?;
    Ok(true)
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
