use super::*;

#[tokio::test]
async fn cleanup_claim_generation_fences_a_stale_worker_after_restart() -> Result<(), String> {
    let (scratch, journal) = open("completion-claim-fence").await?;
    let id = completed_launch(&journal, 86).await?;
    let mut now = 100_i64;
    let mut claim = None;
    for expected in 1_i64..=5 {
        let next = journal
            .claim_completion_cleanup_at(id, now, now + 10)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "expected a cleanup claim".to_owned())?;
        assert_eq!(next.generation, expected);
        claim = Some(next);
        now += 10;
    }
    let stale = claim.ok_or_else(|| "expected the fifth cleanup claim".to_owned())?;
    drop(journal);

    let journal = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let current = journal
        .claim_completion_cleanup_at(id, now, now + 10)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected the sixth cleanup claim".to_owned())?;
    assert_eq!(current.generation, 6);
    assert_eq!(current.attempt, 5);
    assert_eq!(
        journal
            .retry_completion_cleanup_at(id, stale.generation, now, 10_000)
            .await,
        Ok(false),
        "the stale fifth claimant must not change the sixth lease or retry deadline"
    );
    assert_eq!(
        journal
            .claim_completion_cleanup_at(id, now, now + 10)
            .await
            .map_err(|error| error.to_string())?,
        None,
        "the current cleanup lease must remain active"
    );
    let next = journal
        .claim_completion_cleanup_at(id, now + 10, now + 20)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "stale retry changed the current retry deadline".to_owned())?;
    assert_eq!(next.generation, 7);
    Ok(())
}

#[tokio::test]
async fn stale_claim_cannot_prove_or_commit_cleanup_after_reopen() -> Result<(), String> {
    let (scratch, journal) = open("completion-stale-success").await?;
    let id = completed_launch(&journal, 91).await?;
    let mut now = 100_i64;
    let mut stale = None;
    for expected in 1_i64..=5 {
        let claim = journal
            .claim_completion_cleanup_at(id, now, now + 10)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "expected a cleanup claim".to_owned())?;
        assert_eq!(claim.generation, expected);
        stale = Some(claim);
        now += 10;
    }
    let stale = stale.ok_or_else(|| "expected the fifth cleanup claim".to_owned())?;
    drop(journal);

    let journal = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let current = journal
        .claim_completion_cleanup_at(id, now, now + 10)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected claim after restart".to_owned())?;
    assert_eq!(current.generation, 6);
    assert_eq!(
        journal
            .mark_completion_worker_cleanup_proven_at(id, stale.generation, now + 1)
            .await,
        Ok(false)
    );
    assert_eq!(
        journal
            .record_completion_cleanup_at(id, stale.generation, now + 1)
            .await,
        Ok(false)
    );
    assert!(
        !journal
            .completion_worker_cleanup_proven(id)
            .await
            .map_err(|error| error.to_string())?
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    assert_eq!(
        journal
            .claim_completion_cleanup_at(id, now + 1, now + 11)
            .await
            .map_err(|error| error.to_string())?,
        None,
        "stale success must not release the new claim"
    );
    assert!(
        journal
            .mark_completion_worker_cleanup_proven_at(id, current.generation, now + 2)
            .await
            .map_err(|error| error.to_string())?
    );
    assert!(
        journal
            .record_completion_cleanup_at(id, current.generation, now + 3)
            .await
            .map_err(|error| error.to_string())?
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));
    Ok(())
}

#[tokio::test]
async fn cleanup_claim_generation_overflow_fails_closed() -> Result<(), String> {
    let (_scratch, journal) = open("completion-claim-overflow").await?;
    let id = completed_launch(&journal, 87).await?;
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO completion_cleanup (intent_id, attempts, claim_generation, retry_after, lease_until) VALUES (?1, 5, 9223372036854775807, 0, 0)",
            [id],
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.claim_completion_cleanup_at(id, 100, 110).await,
        Err(HostError::Journal),
        "the journal must not reuse a cleanup fencing token"
    );
    Ok(())
}

#[tokio::test]
async fn cleanup_claim_generation_migrates_legacy_attempts() -> Result<(), String> {
    let (scratch, journal) = open("completion-claim-migration").await?;
    let id = completed_launch(&journal, 89).await?;
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("DROP TABLE completion_cleanup", ())
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE completion_cleanup (intent_id INTEGER PRIMARY KEY, attempts INTEGER NOT NULL DEFAULT 0, retry_after INTEGER NOT NULL DEFAULT 0, lease_until INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO completion_cleanup (intent_id, attempts, retry_after, lease_until) VALUES (?1, 5, 0, 0)",
            [id],
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(journal);

    let reopened = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let claim = reopened
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected a claim after the schema migration".to_owned())?;
    assert_eq!(claim.generation, 6);
    assert_eq!(claim.attempt, 5);
    Ok(())
}

#[tokio::test]
async fn completion_cleanup_marker_is_durable_before_archive_retirement() -> Result<(), String> {
    let (scratch, journal) = open("completion-worker-cleanup-proof").await?;
    let id = completed_launch(&journal, 88).await?;
    assert!(
        !journal
            .completion_worker_cleanup_proven(id)
            .await
            .map_err(|error| error.to_string())?
    );
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion cleanup claim".to_owned())?;
    assert!(
        journal
            .mark_completion_worker_cleanup_proven_at(id, claim.generation, 101)
            .await
            .map_err(|error| error.to_string())?
    );
    drop(journal);
    let reopened = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        reopened
            .completion_worker_cleanup_proven(id)
            .await
            .map_err(|error| error.to_string())?
    );
    assert_eq!(reopened.occupied_launches().await, Ok(1));
    Ok(())
}

async fn completed_launch(journal: &crate::Journal, request_id: i64) -> Result<i64, String> {
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, request_id, 1_000 + request_id, 8)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new cleanup-test launch".to_owned());
    };
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected acquire claim".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected JIT claim".to_owned());
    }
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let name = format!("v{}", identity.launch_id());
    journal
        .record_runner_completed(1, request_id, 1_000 + request_id, &name)
        .await
        .map_err(|error| error.to_string())?;
    Ok(id)
}
