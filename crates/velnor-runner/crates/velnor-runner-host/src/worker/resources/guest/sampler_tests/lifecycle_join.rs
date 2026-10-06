use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

use super::super::{GuestProbeIdentity, GuestSampleCache, GuestSamplerTask};
use super::fake::{FakeClient, FakeState, fast_timing};
use super::identity;

#[tokio::test(flavor = "current_thread")]
async fn dropping_task_keeps_async_runtime_responsive_during_cleanup() -> Result<(), Box<dyn Error>>
{
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.remove_delay = Duration::from_millis(500);
    let mut timing = fast_timing();
    timing.cleanup = Duration::from_millis(800);
    timing.attempt = Duration::from_secs(2);
    timing.cleanup_reserve = Duration::from_millis(900);
    let (_cache, task) = spawn_fake_sampler(client.clone(), identity, timing)?;
    wait_for_state(&client, |state| state.remove_count > 0).await?;
    let (heartbeat_sender, heartbeat_receiver) = tokio::sync::oneshot::channel();
    let heartbeat = tokio::spawn(async move {
        sleep(Duration::from_millis(10)).await;
        heartbeat_sender
            .send(())
            .map_err(|_| std::io::Error::other("heartbeat receiver closed"))
    });

    let drop_started = Instant::now();
    drop(task);
    let drop_elapsed = drop_started.elapsed();

    assert!(drop_elapsed < Duration::from_millis(250));
    timeout(Duration::from_millis(150), heartbeat_receiver).await??;
    heartbeat.await??;
    assert!(client.state.lock().await.container.is_some());
    wait_for_state(&client, |state| state.container.is_none()).await?;
    let state = client.state.lock().await;
    assert!(state.container.is_none());
    assert_eq!(state.remove_count, 1);
    Ok(())
}

#[test]
fn cancelled_shutdown_behind_saturated_blocking_pool_keeps_join_guard() -> Result<(), Box<dyn Error>>
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()?;
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());

    runtime.block_on(async {
        client.state.lock().await.remove_delay = Duration::from_millis(500);
        let mut timing = fast_timing();
        timing.cleanup = Duration::from_millis(800);
        timing.attempt = Duration::from_secs(2);
        timing.cleanup_reserve = Duration::from_millis(900);
        let (_cache, task) = spawn_fake_sampler(client.clone(), identity, timing)?;
        wait_for_state(&client, |state| state.remove_count > 0).await?;

        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            started_sender
                .send(())
                .map_err(|_| std::io::Error::other("blocking slot marker closed"))?;
            release_receiver
                .recv()
                .map_err(|_| std::io::Error::other("blocking slot release closed"))?;
            Ok::<(), std::io::Error>(())
        });
        timeout(Duration::from_secs(3), started_receiver).await??;

        assert!(
            timeout(Duration::from_millis(20), task.shutdown())
                .await
                .is_err()
        );
        assert!(client.state.lock().await.container.is_some());
        release_sender
            .send(())
            .map_err(|_| std::io::Error::other("blocking slot receiver closed"))?;
        blocker.await??;
        wait_for_state(&client, |state| state.container.is_none()).await?;
        Ok::<(), Box<dyn Error>>(())
    })?;
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
