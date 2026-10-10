//! Cancellation must stop between effects without blocking the async caller.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tokio::sync::oneshot;

use super::{Harness, Scratch, TEST_REQUEST_ID, completed_launch, completion_now};
use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::completion::cleanup::{
    CLAIM_LEASE_SECONDS, EffectBudget, cleanup_retry_delay_seconds, reconcile_one, run_effect,
};
use crate::launch::completion::{CompletionWorker, ReapTask, Reaper};

const IN_FLIGHT_EFFECT: Duration = Duration::from_millis(300);
const HEARTBEAT_TICK: Duration = Duration::from_millis(10);

#[test]
fn failed_reaper_start_retains_join_until_start_retry() -> Result<(), HostError> {
    let reaper = Reaper::new();
    let (release_worker, worker_release) = mpsc::channel();
    let (worker_done_tx, worker_done) = mpsc::channel();
    let thread = thread::spawn(move || {
        if worker_release.recv().is_err() {
            eprintln!("completion test worker lost phase=release");
            return;
        }
        if worker_done_tx.send(()).is_err() {
            eprintln!("completion test worker lost phase=done");
        }
    });
    let (join_tx, join_rx) = mpsc::channel();
    reaper.enqueue(ReapTask {
        thread,
        finished: None,
        joined: None,
        reaped: Some(join_tx),
    });

    assert!(matches!(
        reaper.ensure_started_with(|_| Err(HostError::Journal)),
        Err(HostError::Journal)
    ));
    assert_eq!(reaper.pending_count(), 1);

    release_worker.send(()).map_err(|_| HostError::Journal)?;
    worker_done
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| HostError::Journal)?;
    reaper.ensure_started()?;
    join_rx
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| HostError::Journal)?;
    assert_eq!(reaper.pending_count(), 0);
    Ok(())
}

#[tokio::test]
async fn stop_after_claim_schedules_durable_retry_without_starting_http() -> Result<(), HostError> {
    let scratch = Scratch::new("stop-before-effect")?;
    let mut harness = Harness::new(scratch.file()).await?;
    let id = completed_launch(&harness.journal, TEST_REQUEST_ID).await?;
    let mut due = harness
        .journal
        .due_completed_launches(completion_now()?, 1)
        .await?;
    let mut launch = due.pop().ok_or(HostError::Journal)?;
    launch.intent.worker_volume = Some("wcompletion".to_owned());
    let second = Journal::open(scratch.file()).await?;
    let stopping = AtomicBool::new(true);
    let mut context = harness.context();

    assert_eq!(reconcile_one(&mut context, launch, &stopping).await, None);

    assert!(
        second
            .claim_completion_cleanup(id, CLAIM_LEASE_SECONDS)
            .await?
            .is_none()
    );
    assert_eq!(
        second.due_completed_launches(completion_now()?, 1).await?,
        [] as [journal::completion::CompletedLaunch; 0]
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_during_an_effect_keeps_heartbeat_and_schedules_retry() -> Result<(), HostError> {
    let InFlightWorker {
        _scratch,
        journal,
        id,
        worker,
        started,
        outcome,
        joined,
        second_effects,
    } = InFlightWorker::start().await?;
    started.await.map_err(|_| HostError::Journal)?;
    let heartbeat = Heartbeat::start();
    tokio::time::sleep(HEARTBEAT_TICK).await;
    let before_drop = heartbeat.count.load(Ordering::Acquire);

    drop(worker);

    assert!(!joined.load(Ordering::Acquire));
    heartbeat.wait_after(before_drop).await?;
    assert!(!joined.load(Ordering::Acquire));
    assert_eq!(second_effects.load(Ordering::Acquire), 0);
    assert!(outcome.await.map_err(|_| HostError::Journal)??);
    wait_for_join(&joined).await?;
    heartbeat.stop().await?;
    assert!(
        journal
            .claim_completion_cleanup(id, CLAIM_LEASE_SECONDS)
            .await?
            .is_none()
    );
    Ok(())
}

struct InFlightWorker {
    _scratch: Scratch,
    journal: Journal,
    id: i64,
    worker: CompletionWorker,
    started: oneshot::Receiver<()>,
    outcome: oneshot::Receiver<Result<bool, HostError>>,
    joined: Arc<AtomicBool>,
    second_effects: Arc<AtomicUsize>,
}

impl InFlightWorker {
    async fn start() -> Result<Self, HostError> {
        let scratch = Scratch::new("drop-in-flight")?;
        let journal = Journal::open(scratch.file()).await?;
        let worker_journal = Journal::open(scratch.file()).await?;
        let id = completed_launch(&journal, TEST_REQUEST_ID).await?;
        let claim = worker_journal
            .claim_completion_cleanup(id, CLAIM_LEASE_SECONDS)
            .await?
            .ok_or(HostError::Journal)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let (wake, wake_receiver) = mpsc::sync_channel(1);
        let (finished_tx, finished) = oneshot::channel();
        let (started_tx, started) = oneshot::channel();
        let (outcome_tx, outcome) = oneshot::channel();
        let joined = Arc::new(AtomicBool::new(false));
        let second_effects = Arc::new(AtomicUsize::new(0));
        let thread = spawn_in_flight_thread(InFlightThread {
            journal: worker_journal,
            id,
            claim,
            stopping: Arc::clone(&stopping),
            second_effects: Arc::clone(&second_effects),
            joined: Arc::clone(&joined),
            wake_receiver,
            started: started_tx,
            outcome: outcome_tx,
            finished: finished_tx,
        });
        Ok(Self {
            _scratch: scratch,
            journal,
            id,
            worker: CompletionWorker {
                wake,
                stopping,
                thread: Some(thread),
                finished: Some(finished),
                reaped: None,
            },
            started,
            outcome,
            joined,
            second_effects,
        })
    }
}

struct InFlightThread {
    journal: Journal,
    id: i64,
    claim: crate::journal::CleanupClaim,
    stopping: Arc<AtomicBool>,
    second_effects: Arc<AtomicUsize>,
    joined: Arc<AtomicBool>,
    wake_receiver: mpsc::Receiver<()>,
    started: oneshot::Sender<()>,
    outcome: oneshot::Sender<Result<bool, HostError>>,
    finished: oneshot::Sender<Result<(), HostError>>,
}

fn spawn_in_flight_thread(task: InFlightThread) -> thread::JoinHandle<()> {
    let InFlightThread {
        journal,
        id,
        claim,
        stopping,
        second_effects,
        joined,
        wake_receiver,
        started,
        outcome,
        finished,
    } = task;
    let generation = claim.generation;
    let attempt = claim.attempt;
    thread::spawn(move || {
        let _wake_receiver = wake_receiver;
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|_| HostError::Journal)
            .map(|runtime| {
                runtime.block_on(run_in_flight_effects(InFlightEffects {
                    journal: &journal,
                    id,
                    claim,
                    generation,
                    attempt,
                    stopping: &stopping,
                    second_effects: &second_effects,
                    started,
                }))
            })
            .and_then(|result| result);
        if outcome.send(result).is_err() {
            eprintln!("completion test worker lost phase=outcome");
        }
        if finished.send(result.map(|_| ())).is_err() {
            eprintln!("completion test worker lost phase=shutdown");
        }
        joined.store(true, Ordering::Release);
    })
}

struct InFlightEffects<'a> {
    journal: &'a Journal,
    id: i64,
    claim: crate::journal::CleanupClaim,
    generation: i64,
    attempt: u32,
    stopping: &'a AtomicBool,
    second_effects: &'a AtomicUsize,
    started: oneshot::Sender<()>,
}

async fn run_in_flight_effects(task: InFlightEffects<'_>) -> Result<bool, HostError> {
    let InFlightEffects {
        journal,
        id,
        claim,
        generation,
        attempt,
        stopping,
        second_effects,
        started,
    } = task;
    let mut budget = EffectBudget::new(stopping);
    run_effect(
        journal,
        &mut budget,
        id,
        claim,
        "bounded HTTP request",
        || async move {
            started.send(()).map_err(|()| HostError::Journal)?;
            tokio::time::sleep(IN_FLIGHT_EFFECT).await;
            Ok(())
        },
    )
    .await
    .map_err(|failure| failure.error)?;
    let second = run_effect(
        journal,
        &mut budget,
        id,
        claim,
        "next HTTP request",
        || async {
            second_effects.fetch_add(1, Ordering::AcqRel);
            Ok(())
        },
    )
    .await;
    let stopped = matches!(second, Err(failure) if failure.stop_requested);
    if !stopped {
        return Err(HostError::Journal);
    }
    if !journal
        .retry_completion_cleanup(id, generation, cleanup_retry_delay_seconds(attempt))
        .await?
    {
        return Err(HostError::Journal);
    }
    Ok(stopped)
}

struct Heartbeat {
    count: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
    thread: tokio::task::JoinHandle<()>,
}

impl Heartbeat {
    fn start() -> Self {
        let count = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let thread_count = Arc::clone(&count);
        let thread_running = Arc::clone(&running);
        let thread = tokio::spawn(async move {
            while thread_running.load(Ordering::Acquire) {
                tokio::time::sleep(HEARTBEAT_TICK).await;
                thread_count.fetch_add(1, Ordering::AcqRel);
            }
        });
        Self {
            count,
            running,
            thread,
        }
    }

    async fn wait_after(&self, before: usize) -> Result<(), HostError> {
        tokio::time::timeout(IN_FLIGHT_EFFECT / 2, async {
            while self.count.load(Ordering::Acquire) == before {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .map_err(|_| HostError::Journal)
    }

    async fn stop(self) -> Result<(), HostError> {
        self.running.store(false, Ordering::Release);
        self.thread.await.map_err(|_| HostError::Journal)
    }
}

async fn wait_for_join(joined: &AtomicBool) -> Result<(), HostError> {
    tokio::time::timeout(IN_FLIGHT_EFFECT, async {
        while !joined.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .map_err(|_| HostError::Journal)
}
