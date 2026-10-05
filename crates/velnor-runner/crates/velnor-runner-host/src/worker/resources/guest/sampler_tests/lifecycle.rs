use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::GuestSampleSnapshot;
use super::GuestSampleFailure;
use super::fake::{FakeClient, fast_sampler, identity, unavailable};

#[tokio::test]
async fn shutdown_waits_for_late_create_reconciliation_and_cleanup() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    {
        let mut state = client.state.lock().await;
        state.create_hangs = true;
        state.late_create_delay = Duration::from_millis(100);
    }
    let initial = GuestSampleSnapshot::completed(Err(GuestSampleFailure::Docker));
    let (sample_sender, _samples) = tokio::sync::watch::channel(initial);
    let (stop_sender, stop_receiver) = tokio::sync::watch::channel(false);
    let sampling = tokio::spawn(super::super::cache::sampling_loop(
        Arc::clone(&client),
        identity,
        Duration::from_secs(1),
        sample_sender,
        stop_receiver,
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if client.state.lock().await.create_count > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    let _previous = stop_sender.send_replace(true);
    tokio::time::timeout(Duration::from_secs(8), sampling).await??;

    let state = client.state.lock().await;
    assert_eq!(state.create_count, 1);
    assert_eq!(state.remove_count, 1);
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn late_exit_inspection_cannot_overrun_the_probe_deadline() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.exit_inspect_delay = Duration::from_millis(40);
    let sampler =
        fast_sampler(Arc::clone(&client), &identity).with_timing(super::super::SamplerTiming {
            operation: Duration::from_millis(100),
            probe_run: Duration::from_millis(25),
            cleanup: Duration::from_millis(25),
            attempt: Duration::from_millis(300),
            cleanup_reserve: Duration::from_millis(100),
            create_reconcile: Duration::from_millis(100),
            exit_poll: Duration::from_millis(1),
        });

    let snapshot = sampler.sample_once().await;

    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    assert_eq!(client.state.lock().await.remove_count, 1);
    Ok(())
}

#[tokio::test]
async fn total_attempt_deadline_bounds_multiple_slow_docker_calls() -> Result<(), Box<dyn Error>> {
    let attempt_budget = Duration::from_millis(120);
    let cleanup_reserve = Duration::from_millis(60);
    let scheduling_tolerance = Duration::from_millis(50);
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.info_delay = Duration::from_millis(15);
    let sampler =
        fast_sampler(Arc::clone(&client), &identity).with_timing(super::super::SamplerTiming {
            operation: Duration::from_millis(25),
            probe_run: Duration::from_millis(50),
            cleanup: Duration::from_millis(25),
            attempt: attempt_budget,
            cleanup_reserve,
            create_reconcile: Duration::from_millis(40),
            exit_poll: Duration::from_millis(1),
        });

    let started = Instant::now();
    let snapshot = sampler.sample_once().await;
    let elapsed = started.elapsed();

    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    // The attempt budget includes reserved cleanup time; allow only scheduler jitter.
    assert!(
        elapsed <= attempt_budget + scheduling_tolerance,
        "sample attempt took {elapsed:?}, exceeding {attempt_budget:?} with {cleanup_reserve:?} cleanup reserve plus {scheduling_tolerance:?} scheduler tolerance"
    );
    assert!(client.state.lock().await.container.is_none());
    Ok(())
}

#[tokio::test]
async fn engine_change_after_owned_id_inspection_prevents_removal() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.switch_engine_after_id_inspect = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert!(unavailable(snapshot, GuestSampleFailure::EngineIdentity));
    assert_eq!(state.remove_count, 0);
    assert!(state.container.is_some());
    Ok(())
}
