//! Bounded, isolated scheduling for completion cleanup.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use velnor_runner_github::Transport;

use crate::journal::Journal;
use crate::listen::Secret;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::PairEngine;

use super::super::capacity;
use super::reconcile;
use super::support::{completion_error, log_cleanup_error, map_journal, retry_delay, unix_seconds};

const CLEANUP_CONCURRENCY_LIMIT: u32 = 4;
const CLEANUP_SCAN_LIMIT: u32 = 128;
const CLEANUP_LEASE_SECONDS: i64 = 120;

static ACTIVE_CLEANUPS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
static ACTIVE_CLEANUP_INTENTS: OnceLock<Arc<Mutex<HashSet<CleanupKey>>>> = OnceLock::new();

pub(in crate::launch) async fn schedule_completed<T, E>(
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
pub(in crate::launch) async fn schedule_completed_isolated<T, E>(
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
        journal: &journal,
        docker: &docker,
        active: &active,
        limit,
    };
    for row in pending {
        if let Some(handle) = schedule.one(row).await? {
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
    ) -> Result<Option<tokio::task::JoinHandle<()>>, EnsureError> {
        let Some(intent_permit) = claim_cleanup_intent(self.journal, row.id)? else {
            return Ok(None);
        };
        if !claim_cleanup_slot(self.active, self.limit) {
            drop(intent_permit);
            return Ok(None);
        }
        let claim = self
            .journal
            .claim_completion_cleanup(row.id, CLEANUP_LEASE_SECONDS)
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
        let task_intent_permit = intent_permit;
        let set_id = self.set_id;
        Ok(Some(tokio::task::spawn_blocking(move || {
            let _permit = CleanupPermit(task_active);
            let _intent_permit = task_intent_permit;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let Ok(runtime) = runtime else {
                return;
            };
            let result = runtime.block_on(reconcile::reconcile_one(
                &mut task_transport,
                set_id,
                task_token.expose(),
                &task_journal,
                &task_docker,
                claim.generation,
                task_row.clone(),
            ));
            if !matches!(result, Ok(true)) {
                if let Ok(retry_after) = retry_delay(claim.attempt) {
                    if let Err(error) = runtime.block_on(task_journal.retry_completion_cleanup(
                        task_row.id,
                        claim.generation,
                        retry_after,
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

fn cleanup_slots() -> Arc<AtomicUsize> {
    Arc::clone(ACTIVE_CLEANUPS.get_or_init(|| Arc::new(AtomicUsize::new(0))))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CleanupKey {
    journal_path: PathBuf,
    row_id: i64,
}

struct CleanupIntentPermit {
    active: Arc<Mutex<HashSet<CleanupKey>>>,
    key: CleanupKey,
}

fn claim_cleanup_intent(
    journal: &Journal,
    row_id: i64,
) -> Result<Option<CleanupIntentPermit>, EnsureError> {
    let active =
        Arc::clone(ACTIVE_CLEANUP_INTENTS.get_or_init(|| Arc::new(Mutex::new(HashSet::new()))));
    let key = CleanupKey {
        journal_path: journal.path().to_path_buf(),
        row_id,
    };
    if !active
        .lock()
        .map_err(|_| completion_error())?
        .insert(key.clone())
    {
        return Ok(None);
    }
    Ok(Some(CleanupIntentPermit { active, key }))
}

impl Drop for CleanupIntentPermit {
    fn drop(&mut self) {
        match self.active.lock() {
            Ok(mut active) => {
                active.remove(&self.key);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(&self.key);
            }
        }
    }
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
