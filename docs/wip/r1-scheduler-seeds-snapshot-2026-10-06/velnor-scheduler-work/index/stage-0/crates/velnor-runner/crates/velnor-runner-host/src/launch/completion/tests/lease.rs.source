use super::*;

#[tokio::test]
async fn missing_archive_lease_keeps_completed_worker_occupied() -> Result<(), String> {
    let (_scratch, journal) = open("completion-missing-lease").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 89).await?;
    journal
        .bind_seed_generation(id, &"a".repeat(64))
        .await
        .map_err(|error| error.to_string())?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    let removed_volumes = engine.removed_volumes.clone();
    let api = BlockingRunnerApi::released(&runner_name, 99);

    completion::record_completion_events(&journal, 7, &completion_poll(89, 99, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let tasks =
        completion::schedule_completed(api.clone(), 7, "admin-token", journal.clone(), engine)
            .await
            .map_err(|error| error.to_string())?;
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }

    assert_eq!(api.calls()?, Vec::new());
    assert!(!removed_volumes.load(Ordering::Acquire));
    assert_eq!(journal.occupied_launches().await, Ok(1));
    assert!(
        !journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    Ok(())
}
