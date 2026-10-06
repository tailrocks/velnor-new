use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

use super::super::{GuestProbeIdentity, GuestSampleCache, GuestSampleFailure, GuestSamplerTask};
use super::fake::{FakeClient, FakeState, fast_sampler, fast_timing, unavailable};
use super::identity;

#[tokio::test]
async fn shutdown_waits_for_late_create_reconciliation_and_cleanup() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    {
        let mut state = client.state.lock().await;
        state.create_hangs = true;
        state.late_create_delay = Duration::from_millis(100);
    }
    let (_cache, task) = spawn_fake_sampler(client.clone(), identity, fast_timing())?;
    wait_for_state(&client, |state| state.create_count > 0).await?;
    shutdown(task).await?;

    let state = client.state.lock().await;
    assert_eq!(state.create_count, 1);
    assert_eq!(state.remove_count, 1);
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn shutdown_during_output_read_waits_for_probe_removal() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.output_hangs = true;
    let (_cache, task) = spawn_fake_sampler(client.clone(), identity, fast_timing())?;
    wait_for_state(&client, |state| state.output_read_count > 0).await?;

    shutdown(task).await?;

    let state = client.state.lock().await;
    assert_eq!(state.remove_count, 1);
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn shutdown_during_remove_waits_until_owned_probe_is_gone() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.remove_delay = Duration::from_millis(60);
    let mut timing = fast_timing();
    timing.cleanup = Duration::from_millis(150);
    timing.attempt = Duration::from_millis(800);
    timing.cleanup_reserve = Duration::from_millis(250);
    let (_cache, task) = spawn_fake_sampler(client.clone(), identity, timing)?;
    wait_for_state(&client, |state| state.remove_count > 0).await?;

    shutdown(task).await?;

    let state = client.state.lock().await;
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn shutdown_after_lost_create_response_cleans_reconciled_probe() -> Result<(), Box<dyn Error>>
{
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.create_loses_response = true;
    let (_cache, task) = spawn_fake_sampler(client.clone(), identity, fast_timing())?;
    wait_for_state(&client, |state| state.create_count > 0).await?;

    shutdown(task).await?;

    let state = client.state.lock().await;
    assert_eq!(state.remove_count, 1);
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn cancelled_shutdown_keeps_joining_late_create_cleanup() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    {
        let mut state = client.state.lock().await;
        state.create_hangs = true;
        state.late_create_delay = Duration::from_millis(100);
    }
    let (_cache, task) = spawn_fake_sampler(client.clone(), identity, fast_timing())?;
    wait_for_state(&client, |state| state.create_count > 0).await?;

    assert!(
        timeout(Duration::from_millis(20), task.shutdown())
            .await
            .is_err()
    );
    wait_for_state(&client, |state| {
        state.create_count > 0 && state.remove_count > 0 && state.container.is_none()
    })
    .await?;
    sleep(Duration::from_millis(20)).await;
    assert_eq!(client.state.lock().await.create_count, 1);
    Ok(())
}

#[tokio::test]
async fn shutdown_returns_failure_when_owned_probe_removal_fails() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.remove_keeps_container = true;
    let (cache, task) = spawn_fake_sampler(client.clone(), identity, fast_timing())?;
    wait_for_sample(&cache).await?;

    assert_eq!(
        cache.latest(Duration::from_secs(5)).status,
        super::super::GuestSampleStatus::Unavailable(GuestSampleFailure::Cleanup)
    );
    let shutdown = timeout(Duration::from_secs(5), task.shutdown()).await?;
    assert_eq!(shutdown, Err(GuestSampleFailure::Cleanup));
    assert!(client.state.lock().await.container.is_some());
    Ok(())
}

#[tokio::test]
async fn slow_cleanup_counts_against_sample_freshness() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.remove_delay = Duration::from_millis(80);
    let mut timing = fast_timing();
    timing.cleanup = Duration::from_millis(150);
    timing.attempt = Duration::from_millis(800);
    timing.cleanup_reserve = Duration::from_millis(250);
    let (cache, task) = spawn_fake_sampler(client, identity, timing)?;
    wait_for_sample(&cache).await?;

    let stale = cache.latest(Duration::from_millis(20));
    assert_eq!(stale.status, super::super::GuestSampleStatus::Stale);
    assert_eq!(stale.sample, super::super::GuestResourceSample::default());
    shutdown(task).await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn sampling_progresses_while_launch_runtime_is_blocked() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    let (cache, task) = spawn_fake_sampler(client, identity, fast_timing())?;

    std::thread::sleep(Duration::from_millis(150));

    assert_eq!(
        cache.latest(Duration::from_secs(1)).status,
        super::super::GuestSampleStatus::Available
    );
    shutdown(task).await?;
    Ok(())
}

#[tokio::test]
async fn runtime_start_failure_is_visible_before_shutdown() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    let (cache, task) = spawn_fake_sampler_with_builder(client, identity, fast_timing(), || {
        Err(GuestSampleFailure::RuntimeUnavailable)
    })?;
    wait_for_sample(&cache).await?;

    assert_eq!(
        cache.latest(Duration::from_secs(5)).status,
        super::super::GuestSampleStatus::Unavailable(GuestSampleFailure::RuntimeUnavailable)
    );
    assert_eq!(
        task.shutdown().await,
        Err(GuestSampleFailure::RuntimeUnavailable)
    );
    Ok(())
}

#[tokio::test]
async fn thread_panic_is_visible_before_shutdown() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    let (cache, task) = spawn_fake_sampler_with_builder(client, identity, fast_timing(), || {
        std::panic::resume_unwind(Box::new("fixture runtime panic"))
    })?;
    wait_for_sample(&cache).await?;

    assert_eq!(
        cache.latest(Duration::from_secs(5)).status,
        super::super::GuestSampleStatus::Unavailable(GuestSampleFailure::SamplerTask)
    );
    assert_eq!(task.shutdown().await, Err(GuestSampleFailure::SamplerTask));
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
    let work_budget = Duration::from_millis(60);
    // Four 15ms engine checks need 60ms; reserve 120ms more for cleanup calls and scheduler jitter.
    let cleanup_reserve = Duration::from_millis(180);
    let attempt_budget = work_budget + cleanup_reserve;
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
async fn insufficient_cleanup_budget_returns_unknown_and_retains_probe()
-> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    {
        let mut state = client.state.lock().await;
        state.exit_inspect_delay = Duration::from_millis(185);
        state.remove_delay = Duration::from_millis(50);
    }
    let timing = super::super::SamplerTiming {
        operation: Duration::from_millis(200),
        probe_run: Duration::from_millis(200),
        cleanup: Duration::from_millis(100),
        attempt: Duration::from_millis(200),
        cleanup_reserve: Duration::from_millis(10),
        ..fast_timing()
    };

    // Exit inspection spends nearly all of the 190ms work window. A 100ms cleanup call budget
    // would permit the 50ms remove, but the 10ms total reserve cannot.
    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .with_timing(timing)
        .sample_once()
        .await;

    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    assert!(client.state.lock().await.container.is_some());
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

fn spawn_fake_sampler(
    client: Arc<FakeClient>,
    identity: GuestProbeIdentity,
    timing: super::super::SamplerTiming,
) -> Result<(GuestSampleCache, GuestSamplerTask), std::io::Error> {
    super::super::cache::spawn_sampling_thread(client, identity, Duration::from_secs(1), timing)
        .map_err(|_| std::io::Error::other("sampler thread did not start"))
}

fn spawn_fake_sampler_with_builder<B>(
    client: Arc<FakeClient>,
    identity: GuestProbeIdentity,
    timing: super::super::SamplerTiming,
    runtime_builder: B,
) -> Result<(GuestSampleCache, GuestSamplerTask), std::io::Error>
where
    B: FnOnce() -> Result<tokio::runtime::Runtime, GuestSampleFailure> + Send + 'static,
{
    super::super::cache::spawn_sampling_thread_with_builder(
        client,
        identity,
        Duration::from_secs(1),
        timing,
        runtime_builder,
    )
    .map_err(|_| std::io::Error::other("sampler thread did not start"))
}

async fn shutdown(task: GuestSamplerTask) -> Result<(), std::io::Error> {
    timeout(Duration::from_secs(5), task.shutdown())
        .await
        .map_err(|_| std::io::Error::other("sampler shutdown timed out"))?
        .map_err(|_| std::io::Error::other("sampler task failed"))
}

async fn wait_for_state(
    client: &FakeClient,
    predicate: impl Fn(&FakeState) -> bool,
) -> Result<(), Box<dyn Error>> {
    timeout(Duration::from_secs(3), async {
        loop {
            let state = client.state.lock().await;
            let matched = predicate(&*state);
            drop(state);
            if matched {
                break;
            }
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    Ok(())
}

async fn wait_for_sample(cache: &GuestSampleCache) -> Result<(), Box<dyn Error>> {
    timeout(Duration::from_secs(3), async {
        loop {
            if cache.latest(Duration::from_secs(5)).status
                != super::super::GuestSampleStatus::Pending
            {
                break;
            }
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    Ok(())
}
