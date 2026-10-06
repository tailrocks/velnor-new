use super::*;

#[tokio::test]
async fn an_expired_live_cleanup_cannot_be_reclaimed_or_delete_github_runner() -> Result<(), String>
{
    let (_scratch, journal) = open("completion-fence-live-task").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 115).await?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    let stale_api = BlockingRunnerApi::new(&runner_name, 125);
    completion::record_completion_events(&journal, 7, &completion_poll(115, 125, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let stale_tasks = completion::schedule_completed_isolated(
        stale_api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    wait_for_blocked_request(&stale_api).await?;
    expire_claim_for_test(&journal, id).await?;

    let duplicate = completion::schedule_completed_isolated(
        stale_api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(duplicate.is_empty(), "a live intent cannot be reclaimed");
    stale_api.release();
    join(stale_tasks).await?;
    let calls = stale_api.calls()?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "GET");
    assert_eq!(
        engine.removal_calls(),
        (0, 0),
        "the stale generation must issue no container or volume deletes"
    );

    let current_api = BlockingRunnerApi::released(&runner_name, 125);
    let current_tasks = completion::schedule_completed_isolated(
        current_api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    join(current_tasks).await?;
    assert!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));
    assert_eq!(
        engine.removal_calls(),
        (2, 3),
        "the current generation must delete two containers and three volumes once"
    );
    assert_eq!(
        current_api
            .calls()?
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        ["GET", "DELETE", "GET"]
    );
    Ok(())
}

#[tokio::test]
async fn a_delayed_delete_is_not_reissued_after_claim_takeover() -> Result<(), String> {
    let (_scratch, journal) = open("completion-fence-delayed-delete").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 116).await?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    let stale_api = BlockingRunnerApi::blocking_delete(&runner_name, 126);
    completion::record_completion_events(&journal, 7, &completion_poll(116, 126, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let stale_tasks = completion::schedule_completed_isolated(
        stale_api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    wait_for_blocked_request(&stale_api).await?;
    expire_claim_for_test(&journal, id).await?;

    let duplicate = completion::schedule_completed_isolated(
        stale_api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(duplicate.is_empty());
    stale_api.release();
    join(stale_tasks).await?;
    assert_eq!(
        stale_api
            .calls()?
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        ["GET", "DELETE"],
        "the already-sent request returns once, then stale cleanup stops"
    );
    assert_eq!(engine.removal_calls(), (0, 0));

    let current_api = BlockingRunnerApi::absent(&runner_name, 126);
    let current_tasks = completion::schedule_completed_isolated(
        current_api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    join(current_tasks).await?;
    assert_eq!(
        current_api
            .calls()?
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        ["GET"],
        "takeover confirms absence and does not repeat DELETE"
    );
    assert_eq!(engine.removal_calls(), (2, 3));
    assert!(journal.intent(id).await.map_err(|error| error.to_string())?.cleanup_proven);
    Ok(())
}

async fn expire_claim_for_test(journal: &Journal, id: i64) -> Result<(), String> {
    journal
        .connection()
        .await
        .map_err(|error| error.to_string())?
        .execute(
            "UPDATE completion_cleanup SET lease_until = 0 WHERE intent_id = ?1",
            [id],
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn join(tasks: Vec<tokio::task::JoinHandle<()>>) -> Result<(), String> {
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }
    Ok(())
}
