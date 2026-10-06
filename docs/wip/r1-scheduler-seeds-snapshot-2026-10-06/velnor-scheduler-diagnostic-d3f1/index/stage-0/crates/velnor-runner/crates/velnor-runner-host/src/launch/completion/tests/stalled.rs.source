use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::launch::capacity::Admit;
use crate::launch::turn;
use crate::launch_harness::available;

use super::*;

#[tokio::test]
async fn stalled_remote_cleanup_does_not_block_a_free_slot() -> Result<(), String> {
    let (_scratch, journal) = open("completion-stalled-lookup").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 85).await?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    let api = BlockingRunnerApi::new(&runner_name, 95);

    completion::record_completion_events(&journal, 7, &completion_poll(85, 95, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let tasks = completion::schedule_completed_isolated(
        api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(started.elapsed() < Duration::from_secs(1));
    wait_for_lookup(&api).await?;

    let admission = turn::admission(&journal, 7, 2, 2, 0, 0, &available(&[86]))
        .await
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        admission.reservation,
        Some(LaunchReservation::New(_))
    ));
    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(journal.occupied_launches().await, Ok(2));
    assert!(
        !journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );

    api.release();
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }
    assert!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    let calls = api.calls()?;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].0, "GET");
    assert_eq!(calls[1].0, "DELETE");
    assert!(calls[1].1.ends_with("/95"));
    assert_eq!(calls[2].0, "GET");
    Ok(())
}

#[test]
fn completion_cleanup_drives_io_while_listener_runtime_is_blocked() -> Result<(), String> {
    let listener = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let (scratch, journal) = listener.block_on(open("completion-listener-blocked"))?;
    let (id, identity, runner_id, dind_id) = listener.block_on(launch(&journal, 7, 87))?;
    let runner_name = format!("v{}", identity.launch_id());
    let (verified, verify_ready) = mpsc::channel();
    let (removed, volume_ready) = mpsc::channel();
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?
        .with_progress_senders(verified, removed)
        .with_verify_timer();
    let removed_volumes = engine.removed_volumes.clone();
    let api = BlockingRunnerApi::released(&runner_name, 97);

    listener
        .block_on(async {
            completion::record_completion_events(
                &journal,
                7,
                &completion_poll(87, 97, &runner_name),
            )
            .await
        })
        .map_err(|error| error.to_string())?;
    let tasks = listener
        .block_on(completion::schedule_completed_isolated(
            api,
            7,
            "admin-token",
            journal.clone(),
            engine,
        ))
        .map_err(|error| error.to_string())?;
    if tasks.len() != 1 {
        return Err("completion cleanup did not claim one row".to_owned());
    }
    // Block this current-thread listener while the cleanup runtime advances its timer and I/O.
    verify_ready
        .recv_timeout(Duration::from_secs(2))
        .map_err(|error| format!("cleanup verify was not reached: {error}"))?;
    volume_ready
        .recv_timeout(Duration::from_secs(2))
        .map_err(|error| format!("cleanup did not remove volumes: {error}"))?;
    let progressed_during_poll = removed_volumes.load(Ordering::Acquire);
    listener.block_on(async {
        for task in tasks {
            task.await.map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    })?;
    assert!(
        progressed_during_poll,
        "Docker cleanup did not advance during the synchronous queue wait"
    );
    assert!(
        listener
            .block_on(journal.intent(id))
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    drop(scratch);
    Ok(())
}
