//! Test worker cleanup, task progress, and retries for one repository connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tokio::sync::oneshot;

use super::completed_fixture;
use crate::error::HostError;
use crate::journal::CleanupClaim;
use crate::launch::completion::CompletionWorker;
use crate::launch::completion::cleanup::{
    CLAIM_LEASE_SECONDS, EffectBudget, cleanup_retry_delay_seconds, completion_now, run_effect,
};
use crate::launch::completion::journal_ref::CompletionJournal;

#[test]
fn dropping_scoped_worker_during_effect_keeps_heartbeat_and_retries() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "dropping_scoped_worker_during_effect_keeps_heartbeat_and_retries",
        || Box::pin(dropping_scoped_worker_during_effect_inner()),
    )
}

async fn dropping_scoped_worker_during_effect_inner() -> Result<(), String> {
    let fixture = completed_fixture()
        .await
        .map_err(|error| error.to_string())?;
    let now = completion_now().map_err(|error| error.to_string())?;
    let id = fixture.intent_id;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(id, now, CLAIM_LEASE_SECONDS)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "The completion claim for this repository connection is absent.".to_owned()
        })?;
    let stopping = Arc::new(AtomicBool::new(false));
    let completion = CompletionJournal::scoped(fixture.scope.clone());
    let generation = claim.generation;
    let attempt = claim.attempt;
    let effects = Arc::new(AtomicUsize::new(0));
    let worker_state = spawn_blocked_worker(WorkerEffectContext {
        completion,
        scope: fixture.scope.clone(),
        stopping,
        id,
        claim,
        second_effects: Arc::clone(&effects),
        generation,
        attempt,
    });
    let retry_window_start = completion_now().map_err(|error| error.to_string())?;
    verify_drop_during_effect(worker_state, &effects).await?;
    verify_retry_boundary(&fixture, id, attempt, retry_window_start).await
}

async fn verify_drop_during_effect(
    mut worker_state: BlockedWorker,
    effects: &AtomicUsize,
) -> Result<(), String> {
    worker_state
        .started
        .await
        .map_err(|error| error.to_string())?;
    drop(worker_state.worker);
    let heartbeat = Arc::new(AtomicUsize::new(0));
    let ticker = Arc::clone(&heartbeat);
    let task = tokio::spawn(async move {
        for _ in 0..8 {
            ticker.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert!(heartbeat.load(Ordering::Relaxed) > 0);
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    assert!(worker_state.done.try_recv().is_err());
    worker_state
        .release
        .send(())
        .map_err(|()| "The active request ended too soon.".to_owned())?;
    task.await.map_err(|error| error.to_string())?;
    worker_state.done.await.map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || worker_state.joined.recv_timeout(Duration::from_secs(2)))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    Ok(())
}

async fn verify_retry_boundary(
    fixture: &super::NativeStartedIntakeFixture,
    id: i64,
    attempt: u32,
    retry_window_start: i64,
) -> Result<(), String> {
    let retry_delay = cleanup_retry_delay_seconds(attempt);
    let retry_at = retry_window_start
        .checked_add(retry_delay)
        .ok_or_else(|| "The retry time is too large.".to_owned())?;
    assert!(
        fixture
            .scope
            .claim_completion_cleanup_at(id, retry_at - 1, CLAIM_LEASE_SECONDS)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert!(
        fixture
            .scope
            .claim_completion_cleanup_at(id, retry_at + 5, CLAIM_LEASE_SECONDS)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
    );
    Ok(())
}

struct BlockedWorker {
    worker: CompletionWorker,
    started: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    joined: mpsc::Receiver<()>,
    done: oneshot::Receiver<()>,
}

struct WorkerEffectContext {
    completion: CompletionJournal,
    scope: crate::journal::ConnectionJournal,
    stopping: Arc<AtomicBool>,
    id: i64,
    claim: CleanupClaim,
    second_effects: Arc<AtomicUsize>,
    generation: i64,
    attempt: u32,
}

fn spawn_blocked_worker(context: WorkerEffectContext) -> BlockedWorker {
    let WorkerEffectContext {
        completion,
        scope,
        stopping,
        id,
        claim,
        second_effects,
        generation,
        attempt,
    } = context;
    let thread_stopping = Arc::clone(&stopping);
    let (wake, _wake_receiver) = mpsc::sync_channel(1);
    let (finished_tx, finished) = oneshot::channel();
    let (started_tx, started) = oneshot::channel();
    let (release_tx, release) = oneshot::channel();
    let (joined_tx, joined_rx) = mpsc::channel();
    let (done_tx, done) = oneshot::channel();
    let thread_second_effects = Arc::clone(&second_effects);
    let worker_thread = thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| HostError::Journal)
            .and_then(|runtime| {
                runtime.block_on(run_worker_effects(WorkerEffectRun {
                    completion,
                    scope,
                    stopping: thread_stopping,
                    id,
                    claim,
                    second_effects: thread_second_effects,
                    generation,
                    attempt,
                    started: started_tx,
                    release,
                }))
            });
        let _sent_finished = finished_tx.send(result);
        let _sent_done = done_tx.send(());
    });
    BlockedWorker {
        worker: CompletionWorker {
            wake,
            stopping,
            thread: Some(worker_thread),
            finished: Some(finished),
            reaped: Some(joined_tx),
        },
        started,
        release: release_tx,
        joined: joined_rx,
        done,
    }
}

struct WorkerEffectRun {
    completion: CompletionJournal,
    scope: crate::journal::ConnectionJournal,
    stopping: Arc<AtomicBool>,
    id: i64,
    claim: CleanupClaim,
    second_effects: Arc<AtomicUsize>,
    generation: i64,
    attempt: u32,
    started: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

async fn run_worker_effects(run: WorkerEffectRun) -> Result<(), HostError> {
    let WorkerEffectRun {
        completion,
        scope,
        stopping,
        id,
        claim,
        second_effects,
        generation,
        attempt,
        started,
        release,
    } = run;
    let mut budget = EffectBudget::new(&stopping);
    run_effect(
        &completion,
        &mut budget,
        id,
        claim,
        "in-flight scoped request",
        || async move {
            started.send(()).map_err(|()| HostError::Journal)?;
            release.await.map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await
    .map_err(|failure| failure.error)?;
    let failure = run_effect(
        &completion,
        &mut budget,
        id,
        claim,
        "next scoped request",
        || async move {
            second_effects.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    )
    .await
    .err()
    .ok_or(HostError::Journal)?;
    if !failure.stop_requested {
        return Err(HostError::Journal);
    }
    scope
        .retry_completion_cleanup(id, generation, cleanup_retry_delay_seconds(attempt))
        .await
        .and_then(|scheduled| scheduled.then_some(()).ok_or(HostError::Journal))
}
