//! Completion cleanup claim checks for Docker identity binding.

use crate::Journal;

#[tokio::test]
async fn stale_completion_claim_cannot_bind_docker_ids() -> Result<(), String> {
    let (_scratch, journal, id) = setup("completion-bind-stale").await?;
    let stale = journal
        .claim_completion_cleanup_at(id, 100, 10)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected first completion claim".to_owned())?;
    let current = journal
        .claim_completion_cleanup_at(id, 110, 120)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected replacement completion claim".to_owned())?;
    assert_eq!(current.generation, stale.generation + 1);
    assert_eq!(
        journal
            .bind_completion_containers_at(
                id,
                stale.generation,
                Some("runner-container-stale"),
                None,
                111,
            )
            .await,
        Ok(false)
    );
    let row = journal
        .intent(id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(row.docker_id, None);
    assert_eq!(row.dind_id, None);
    Ok(())
}

#[tokio::test]
async fn current_completion_claim_binds_the_observed_pair_atomically() -> Result<(), String> {
    let (_scratch, journal, id) = setup("completion-bind-pair").await?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion claim".to_owned())?;
    assert!(
        journal
            .bind_completion_containers_at(
                id,
                claim.generation,
                Some("runner-container-current"),
                Some("dind-container-current"),
                101,
            )
            .await
            .map_err(|error| error.to_string())?
    );
    assert_eq!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .docker_id
            .as_deref(),
        Some("runner-container-current")
    );
    assert_eq!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .dind_id
            .as_deref(),
        Some("dind-container-current")
    );
    Ok(())
}

async fn setup(label: &str) -> Result<(crate::launch_harness::Scratch, Journal, i64), String> {
    let scratch = crate::launch_harness::Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("docker-engine-bind-fence")
        .await
        .map_err(|error| error.to_string())?;
    let id = completed_launch(&journal).await?;
    Ok((scratch, journal, id))
}

async fn completed_launch(journal: &Journal) -> Result<i64, String> {
    let (id, _) = journal
        .begin_assigned_launch("m900r146", 7, 146, "v146")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .record_runner_completed(7, 146, 1_146, "v146")
            .await
            .map_err(|error| error.to_string())?,
        Some(id)
    );
    Ok(id)
}
