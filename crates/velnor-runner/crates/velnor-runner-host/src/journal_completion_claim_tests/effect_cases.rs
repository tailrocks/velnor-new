use super::*;

#[tokio::test]
async fn effect_renewal_blocks_reclaim_until_response_and_backoff() -> Result<(), HostError> {
    let scratch = Scratch::new("effect-renewal")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 20)
        .await?
        .ok_or(HostError::Journal)?;
    let second_handle = Journal::open(&scratch.file()).await?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_journal = journal.clone();
    let generation = first.generation;
    let effect = tokio::spawn(async move {
        running_journal
            .run_completion_cleanup_effect_at(id, generation, 119, 120, move || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Ok(())
            })
            .await
    });
    started_rx.await.map_err(|_| HostError::Journal)?;
    assert!(
        second_handle
            .claim_completion_cleanup_at(id, 238, 10)
            .await?
            .is_none()
    );
    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    assert_eq!(effect.await.map_err(|_| HostError::Journal)??, Some(()));
    assert!(
        second_handle
            .retry_completion_cleanup_at(id, generation, 238, 240)
            .await?
    );
    assert!(
        second_handle
            .claim_completion_cleanup_at(id, 239, 10)
            .await?
            .is_none()
    );
    let next = second_handle
        .claim_completion_cleanup_at(id, 240, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(next.generation, generation + 1);
    Ok(())
}

#[tokio::test]
async fn overlapping_effects_keep_the_longest_claim_deadline() -> Result<(), HostError> {
    let scratch = Scratch::new("overlapping-effects")?;
    let journal = Journal::open(&scratch.file()).await?;
    let second_handle = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 20)
        .await?
        .ok_or(HostError::Journal)?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let running_journal = journal.clone();
    let generation = first.generation;
    let first_effect = tokio::spawn(async move {
        running_journal
            .run_completion_cleanup_effect_at(id, generation, 119, 120, move || async move {
                started_tx.send(()).map_err(|()| HostError::Journal)?;
                finish_rx.await.map_err(|_| HostError::Journal)?;
                Ok(())
            })
            .await
    });
    started_rx.await.map_err(|_| HostError::Journal)?;

    let shorter = second_handle
        .run_completion_cleanup_effect_at(id, generation, 110, 10, || async { Ok(()) })
        .await?;
    assert_eq!(shorter, Some(()));
    assert!(
        second_handle
            .claim_completion_cleanup_at(id, 238, 10)
            .await?
            .is_none()
    );

    finish_tx.send(()).map_err(|()| HostError::Journal)?;
    assert_eq!(
        first_effect.await.map_err(|_| HostError::Journal)??,
        Some(())
    );
    let reclaimed = second_handle
        .claim_completion_cleanup_at(id, 239, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reclaimed.generation, generation + 1);
    Ok(())
}

#[tokio::test]
async fn stale_expired_invalid_and_overflow_effects_do_not_run() -> Result<(), HostError> {
    let scratch = Scratch::new("effect-rejection")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let claim = journal
        .claim_completion_cleanup_at(id, 10, 10)
        .await?
        .ok_or(HostError::Journal)?;
    let calls = AtomicUsize::new(0);
    let stale = journal
        .run_completion_cleanup_effect_at(id, claim.generation + 1, 11, 10, || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    let expired = journal
        .run_completion_cleanup_effect_at(id, claim.generation, 20, 10, || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await?;
    assert!(stale.is_none());
    assert!(expired.is_none());
    assert_eq!(
        journal
            .run_completion_cleanup_effect_at(id, claim.generation, 11, 0, || async {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal
            .run_completion_cleanup_effect_at(id, claim.generation, i64::MAX - 1, 2, || async {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },)
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
async fn final_proof_is_claim_fenced_and_releases_uncertain_capacity() -> Result<(), HostError> {
    let scratch = Scratch::new("final-proof")?;
    let journal = Journal::open(&scratch.file()).await?;
    let (id, _) = journal
        .begin_assigned_launch("m1r62", 77, 62, "v62")
        .await?;
    assert!(journal.claim_assigned_acquire(id).await?);
    journal.record_assigned_acquire(id, true).await?;
    journal.finish(id, Outcome::Uncertain).await?;
    assert_eq!(
        journal.record_runner_completed(77, 62, 902, "v62").await?,
        Some(id)
    );
    let before = journal.rows().await?.remove(0);
    assert!(occupies(&before));
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        journal
            .bind_completion_containers_at(
                id,
                claim.generation,
                Some("ctr-runner"),
                Some("ctr-dind"),
                101
            )
            .await?
    );
    assert!(
        !journal
            .bind_completion_containers_at(id, claim.generation, Some("ctr-other"), None, 102)
            .await?
    );
    assert!(
        !journal
            .record_completion_runner_absent_at(id, claim.generation + 1, 103)
            .await?
    );
    assert!(
        !journal
            .record_completion_runner_absent_at(id, claim.generation, 110)
            .await?
    );
    assert!(
        !journal
            .record_completion_cleanup_at(id, claim.generation, 109)
            .await?
    );
    assert!(
        journal
            .record_completion_runner_absent_at(id, claim.generation, 109)
            .await?
    );
    assert!(
        journal
            .record_completion_cleanup_at(id, claim.generation, 109)
            .await?
    );
    let after = Journal::open(&scratch.file())
        .await?
        .rows()
        .await?
        .remove(0);
    assert!(after.cleanup_proven);
    assert!(!occupies(&after));
    assert_eq!(after.state, crate::IntentState::Uncertain);
    Ok(())
}

#[tokio::test]
async fn general_cleanup_cannot_release_a_completion_row() -> Result<(), HostError> {
    let scratch = Scratch::new("general-cleanup-fence")?;
    let journal = Journal::open(&scratch.file()).await?;
    let id = completed_launch(&journal).await?;
    let second_handle = Journal::open(&scratch.file()).await?;

    assert_eq!(
        second_handle.record_cleanup(id).await,
        Err(HostError::Journal)
    );
    let rows = second_handle.rows().await?;
    assert!(occupies(rows.first().ok_or(HostError::Journal)?));
    assert!(!rows[0].cleanup_proven);
    let due = second_handle.due_completed_launches(0, 10).await?;
    assert_eq!(due.len(), 1);
    assert!(!due[0].identity.runner_absent);
    Ok(())
}
