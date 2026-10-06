//! Keep reaper ownership when shutdown is canceled under pool saturation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tokio::sync::oneshot;

use crate::error::HostError;
use crate::launch::completion::CompletionWorker;

#[test]
fn cancelled_shutdown_keeps_join_ownership_with_blocking_pool_saturated() -> Result<(), HostError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .map_err(|_| HostError::Journal)?;
    let saturated = SaturatedWorker::start();
    let SaturatedWorker {
        worker,
        stopping,
        result_sent,
        release_worker,
        worker_done,
        reaped,
    } = saturated;
    let (release_pool, pool_done, pool_task) =
        runtime.block_on(cancel_with_saturated_pool(worker, stopping, result_sent))?;
    runtime.shutdown_background();
    release_pool.send(()).map_err(|_| HostError::Journal)?;
    pool_done
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| HostError::Journal)?;
    wait_for_pool_task(&pool_task)?;
    drop(pool_task);
    release_worker.send(()).map_err(|_| HostError::Journal)?;
    worker_done
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| HostError::Journal)?;
    reaped
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

struct SaturatedWorker {
    worker: CompletionWorker,
    stopping: Arc<AtomicBool>,
    result_sent: Arc<AtomicBool>,
    release_worker: mpsc::Sender<()>,
    worker_done: mpsc::Receiver<()>,
    reaped: mpsc::Receiver<()>,
}

impl SaturatedWorker {
    fn start() -> Self {
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let result_sent = Arc::new(AtomicBool::new(false));
        let thread_result_sent = Arc::clone(&result_sent);
        let (wake, wake_receiver) = mpsc::sync_channel(1);
        let (finished_tx, finished) = oneshot::channel();
        let (release_worker, release_worker_rx) = mpsc::channel();
        let (worker_done_tx, worker_done) = mpsc::channel();
        let (reaped_tx, reaped) = mpsc::channel();
        let thread = thread::spawn(move || {
            if !wait_for_stop_signal(&wake_receiver, &thread_stopping) {
                return;
            }
            if finished_tx.send(Ok(())).is_err() {
                eprintln!("completion test worker lost phase=shutdown");
                return;
            }
            thread_result_sent.store(true, Ordering::Release);
            wait_for_worker_release(&release_worker_rx);
            if worker_done_tx.send(()).is_err() {
                eprintln!("completion test worker lost phase=done");
            }
        });
        Self {
            worker: CompletionWorker {
                wake,
                stopping: Arc::clone(&stopping),
                thread: Some(thread),
                finished: Some(finished),
                reaped: Some(reaped_tx),
            },
            stopping,
            result_sent,
            release_worker,
            worker_done,
            reaped,
        }
    }
}

fn wait_for_stop_signal(receiver: &mpsc::Receiver<()>, stopping: &AtomicBool) -> bool {
    let signaled = receiver.recv_timeout(Duration::from_secs(2)).is_ok();
    if !signaled || !stopping.load(Ordering::Acquire) {
        eprintln!("completion test worker timed out phase=stop");
        return false;
    }
    true
}

fn wait_for_worker_release(receiver: &mpsc::Receiver<()>) {
    if receiver.recv_timeout(Duration::from_secs(2)).is_err() {
        eprintln!("completion test worker timed out phase=in_flight");
    }
}

async fn cancel_with_saturated_pool(
    worker: CompletionWorker,
    stopping: Arc<AtomicBool>,
    result_sent: Arc<AtomicBool>,
) -> Result<
    (
        mpsc::Sender<()>,
        mpsc::Receiver<()>,
        tokio::task::JoinHandle<()>,
    ),
    HostError,
> {
    let (pool_release, pool_release_rx) = mpsc::channel();
    let (pool_started_tx, pool_started_rx) = oneshot::channel();
    let (pool_done_tx, pool_done) = mpsc::channel();
    let pool_task = spawn_pool_blocker(pool_started_tx, pool_release_rx, pool_done_tx);
    pool_started_rx.await.map_err(|_| HostError::Journal)?;

    let shutdown = tokio::spawn(worker.shutdown());
    while !stopping.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }
    wait_for_result(&result_sent).await?;
    tokio::time::sleep(Duration::from_millis(25)).await;
    shutdown.abort();
    let result = shutdown.await;
    if !matches!(result, Err(error) if error.is_cancelled()) {
        return Err(HostError::Journal);
    }
    Ok((pool_release, pool_done, pool_task))
}

fn spawn_pool_blocker(
    started: oneshot::Sender<()>,
    release: mpsc::Receiver<()>,
    finished: mpsc::Sender<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        if started.send(()).is_err() || release.recv().is_err() {
            return;
        }
        if finished.send(()).is_err() {
            eprintln!("completion test blocking task lost phase=done");
        }
    })
}

fn wait_for_pool_task(task: &tokio::task::JoinHandle<()>) -> Result<(), HostError> {
    for _ in 0..100 {
        if task.is_finished() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(HostError::Journal)
}

async fn wait_for_result(result_sent: &AtomicBool) -> Result<(), HostError> {
    tokio::time::timeout(Duration::from_secs(1), async {
        while !result_sent.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .map_err(|_| HostError::Journal)
}
