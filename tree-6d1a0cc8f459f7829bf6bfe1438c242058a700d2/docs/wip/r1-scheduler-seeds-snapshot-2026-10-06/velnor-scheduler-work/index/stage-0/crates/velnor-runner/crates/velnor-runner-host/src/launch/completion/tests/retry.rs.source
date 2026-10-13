use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::launch::capacity::Admit;
use crate::launch::turn;
use crate::launch_harness::available;

use super::*;

#[tokio::test]
async fn cleanup_failure_retries_and_refills_the_capacity_wave() -> Result<(), String> {
    let (scratch, journal) = open("completion-retry-refill").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 88).await?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    engine.fail_volumes.store(true, Ordering::Release);
    let api = BlockingRunnerApi::released(&runner_name, 98);

    completion::record_completion_events(&journal, 7, &completion_poll(88, 98, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let attempt_started_at = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
    )
    .map_err(|error| error.to_string())?;
    let first = completion::schedule_completed(
        api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    for task in first {
        task.await.map_err(|error| error.to_string())?;
    }
    assert_eq!(journal.occupied_launches().await, Ok(1));
    assert!(
        !journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );

    engine.fail_volumes.store(false, Ordering::Release);
    drop(journal);
    let journal = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .due_completed_launches(attempt_started_at, 8)
            .await
            .map_err(|error| error.to_string())?
            .is_empty(),
        "restart discarded the durable cleanup backoff"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let retry = completion::schedule_completed(api, 7, "admin-token", journal.clone(), engine)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(retry.len(), 1);
    for task in retry {
        task.await.map_err(|error| error.to_string())?;
    }
    assert!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));

    let admission = turn::admission(&journal, 7, 1, 1, 0, 0, &available(&[90]))
        .await
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        admission.reservation,
        Some(LaunchReservation::New(_))
    ));
    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(journal.occupied_launches().await, Ok(1));
    Ok(())
}
