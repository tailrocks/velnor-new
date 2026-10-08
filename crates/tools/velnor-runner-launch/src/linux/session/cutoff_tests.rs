use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::sleep;
use velnor_runner_host::Journal;

use super::super::ShutdownGate;
use super::{DispatchFence, await_before_deadline, bounded, bounded_persisting, bounded_protocol};
use crate::launch::harness::Scratch;

#[tokio::test]
async fn published_expired_cutoff_prevents_phase_dispatch() {
    let cutoff = Instant::now()
        .checked_sub(Duration::from_millis(1))
        .expect("monotonic clock has a prior instant");
    let (_sender, mut receiver) = watch::channel(Some(cutoff));
    let mut retained = None;
    let mut gate = ShutdownGate {
        receiver: &mut receiver,
        cutoff: &mut retained,
    };
    let dispatched = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&dispatched);

    let result = bounded(&mut gate, Duration::from_secs(30), None, async move {
        observed.store(true, Ordering::SeqCst);
        7
    })
    .await;

    assert_eq!(result, None);
    assert!(!dispatched.load(Ordering::SeqCst));
    assert_eq!(retained, Some(cutoff));
}

#[tokio::test]
async fn mid_phase_signal_cancels_wait_at_its_absolute_cutoff() -> Result<(), String> {
    let (sender, mut receiver) = watch::channel(None);
    let dispatched = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(AtomicBool::new(false));
    let observed_dispatch = Arc::clone(&dispatched);
    let observed_completion = Arc::clone(&completed);
    let task = tokio::spawn(async move {
        let mut retained = None;
        let mut gate = ShutdownGate {
            receiver: &mut receiver,
            cutoff: &mut retained,
        };
        let result = bounded(&mut gate, Duration::from_secs(30), None, async move {
            observed_dispatch.store(true, Ordering::SeqCst);
            sleep(Duration::from_secs(5)).await;
            observed_completion.store(true, Ordering::SeqCst);
            9
        })
        .await;
        (result, retained)
    });

    while !dispatched.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    let cutoff = Instant::now()
        .checked_add(Duration::from_millis(40))
        .expect("monotonic clock supports a short deadline");
    sender
        .send(Some(cutoff))
        .map_err(|_| "shutdown receiver closed")?;
    let (result, retained) = task.await.map_err(|error| error.to_string())?;

    assert_eq!(result, None);
    assert_eq!(retained, Some(cutoff));
    assert!(!completed.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn signal_during_owned_phase_persists_drain_before_cutoff() -> Result<(), String> {
    let scratch = Scratch::new("linux-cutoff-durable-fence").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (sender, mut receiver) = watch::channel(None);
    let cutoff = Instant::now()
        .checked_add(Duration::from_millis(200))
        .ok_or("monotonic clock supports a short deadline")?;
    sender
        .send(Some(cutoff))
        .map_err(|_| "shutdown receiver closed")?;
    let mut retained = None;
    let mut gate = ShutdownGate {
        receiver: &mut receiver,
        cutoff: &mut retained,
    };
    let result = bounded_persisting(
        &journal,
        &mut gate,
        Duration::from_secs(2),
        None,
        sleep(Duration::from_secs(5)),
    )
    .await;

    assert_eq!(result, None);
    assert_eq!(retained, Some(cutoff));
    assert!(
        journal
            .draining()
            .await
            .map_err(|error| error.to_string())?
    );
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn queued_blocking_dispatch_cannot_begin_after_phase_cutoff() -> Result<(), String> {
    let scratch =
        Scratch::new("linux-cutoff-queued-dispatch").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (_sender, mut receiver) = watch::channel(None);
    let worker_shutdown = receiver.clone();
    let mut retained = None;
    let mut gate = ShutdownGate {
        receiver: &mut receiver,
        cutoff: &mut retained,
    };
    let dispatch = DispatchFence::new();
    let worker_dispatch = dispatch.clone();
    let (completed, observed) = tokio::sync::oneshot::channel();
    let operation = async move {
        let _worker = tokio::task::spawn_blocking(move || {
            std::thread::sleep(Duration::from_millis(80));
            let began = worker_dispatch.begin(&worker_shutdown, None);
            match completed.send(began) {
                Ok(()) | Err(_) => {}
            }
        });
        std::future::pending::<()>().await;
    };
    let phase_deadline = Instant::now()
        .checked_add(Duration::from_millis(30))
        .ok_or("monotonic clock supports a short deadline")?;

    let result = bounded_protocol(
        &journal,
        &mut gate,
        Duration::from_secs(1),
        Some(phase_deadline),
        dispatch,
        operation,
    )
    .await;
    assert_eq!(result, None);
    assert!(
        !tokio::time::timeout(Duration::from_secs(1), observed)
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?
    );
    drop(journal);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn blocked_journal_observation_cannot_extend_phase_deadline() {
    let phase_deadline = Instant::now()
        .checked_add(Duration::from_millis(30))
        .unwrap_or_else(Instant::now);
    let started = Instant::now();
    let observed =
        await_before_deadline(Some(phase_deadline), std::future::pending::<bool>()).await;

    assert_eq!(observed, None);
    assert!(started.elapsed() < Duration::from_millis(500));
}
