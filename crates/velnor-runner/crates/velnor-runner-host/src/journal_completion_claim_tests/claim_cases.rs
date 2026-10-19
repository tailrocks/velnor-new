use super::*;

#[tokio::test]
async fn production_claim_check_rejects_expiry_and_generation_change() -> Result<(), HostError> {
    let scratch = Scratch::new("production-current-check")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let claim = journal
        .claim_completion_cleanup(id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        journal
            .completion_cleanup_claim_current(id, claim.generation)
            .await?
    );
    expire_claim(&scratch.file(), id).await?;
    assert!(
        !journal
            .completion_cleanup_claim_current(id, claim.generation)
            .await?
    );
    let renewed = journal
        .claim_completion_cleanup(id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(renewed.generation > claim.generation);
    assert!(
        !journal
            .completion_cleanup_claim_current(id, claim.generation)
            .await?
    );
    assert!(
        journal
            .completion_cleanup_claim_current(id, renewed.generation)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn claims_fence_retries_and_survive_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new("retry")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first.generation, 1);
    assert_eq!(first.attempt, 1);
    assert_eq!(journal.due_completed_launches(100, 10).await?.first(), None);
    assert!(
        journal
            .record_completion_runner_absent_at(id, first.generation, 109)
            .await?
    );
    assert!(
        journal
            .retry_completion_cleanup_at(id, first.generation, 109, 119)
            .await?
    );
    assert!(
        !journal
            .completion_cleanup_claim_current_at(id, first.generation, 109)
            .await?
    );
    assert_eq!(journal.due_completed_launches(118, 10).await?.first(), None);
    let due = journal.due_completed_launches(119, 10).await?;
    assert_eq!(due.len(), 1);
    assert!(due[0].identity.runner_absent);
    let reopened = Journal::open(&scratch.file()).await?;
    let second = reopened
        .claim_completion_cleanup_at(id, 119, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(second.generation, 2);
    assert_eq!(second.attempt, 2);
    assert!(
        !reopened
            .retry_completion_cleanup_at(id, first.generation, 120, 120)
            .await?
    );
    assert!(
        reopened
            .completion_cleanup_claim_current_at(id, second.generation, 120)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn claim_generation_never_reuses_or_wraps_at_integer_max() -> Result<(), HostError> {
    let scratch = Scratch::new("generation-overflow")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    set_claim_generation(&scratch.file(), id, i64::MAX).await?;
    assert_eq!(
        journal.claim_completion_cleanup_at(id, 10, 10).await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn retry_attempt_backoff_caps_while_generation_keeps_increasing() -> Result<(), HostError> {
    let scratch = Scratch::new("attempt-cap")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    for (index, ordinal) in (1..=7).enumerate() {
        let now = i64::try_from(index).map_err(|_| HostError::Journal)?;
        let claim = journal
            .claim_completion_cleanup_at(id, now, 1)
            .await?
            .ok_or(HostError::Journal)?;
        assert_eq!(claim.generation, i64::from(ordinal));
        let expected_attempt = u32::try_from(ordinal.min(5)).map_err(|_| HostError::Journal)?;
        assert_eq!(claim.attempt, expected_attempt);
    }
    Ok(())
}
