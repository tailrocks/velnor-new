use super::*;

#[tokio::test]
async fn v36_progressed_claim_survives_startup_and_expired_takeover() -> Result<(), HostError> {
    Box::pin(v36_progressed_claim_survives_startup_and_expired_takeover_inner()).await
}

async fn v36_progressed_claim_survives_startup_and_expired_takeover_inner() -> Result<(), HostError>
{
    let fixture = Box::pin(native_started_intake_fixture()).await?;
    let connection = fixture.journal.connection().await?;
    downgrade_retirement_guard_to_v36(&connection).await?;
    let (reservation_id, snapshot) =
        Box::pin(insert_v36_live_claim(&connection, &fixture, 60)).await?;
    let claim = Box::pin(
        fixture
            .scope
            .persisted_post_jit_retirement_claim_for_test(reservation_id),
    )
    .await?;
    let live_candidates = Box::pin(fixture.scope.recoverable_post_jit_retirements()).await?;
    if !live_candidates.is_empty()
        || claim.lease_until() <= scalar(&connection, "SELECT unixepoch()").await?
    {
        return Err(HostError::Identity);
    }
    drop(connection);

    let proof = Box::pin(crate::launch::bootstrap::verify_storage_cleanup_for_scope(
        &fixture.scope,
        &fixture.journal,
        reservation_id,
    ))
    .await?;
    Box::pin(fixture.scope.record_retirement_local_absence(&claim, proof)).await?;
    let mut transport = EmptyRunnerTransport;
    let receipt = ensure_runner_absent(&mut transport, &fixture.runner, "test-admin-token")
        .map_err(|_| HostError::Identity)?;
    Box::pin(
        fixture
            .scope
            .record_mocked_retirement_remote_absence(&claim, &receipt),
    )
    .await?;
    Box::pin(fixture.scope.record_retirement_storage_phase(&claim)).await?;
    assert_phase3_is_charged(&fixture.journal, reservation_id, 1, &snapshot).await?;

    let path = fixture.database_path.clone();
    let migrated = Box::pin(Journal::open(&path)).await?;
    let connection = migrated.connection().await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 37);
    assert_phase3_is_charged(&migrated, reservation_id, 1, &snapshot).await?;
    drop(connection);
    drop(migrated);

    Box::pin(
        crate::journal::connection::reservation::post_jit_retirement::test_support::recover_fixture(
            &fixture,
        ),
    )
    .await?;
    assert_phase3_is_charged(&fixture.journal, reservation_id, 1, &snapshot).await?;
    let mut candidates = Box::pin(fixture.scope.recoverable_post_jit_retirements()).await?;
    if candidates.is_empty() {
        let clock = fixture.journal.connection().await?;
        let now = scalar(&clock, "SELECT unixepoch()").await?;
        let remaining = claim
            .lease_until()
            .saturating_sub(now)
            .saturating_add(1)
            .max(0);
        tokio::time::sleep(std::time::Duration::from_secs(remaining as u64)).await;
        candidates = Box::pin(fixture.scope.recoverable_post_jit_retirements()).await?;
    }
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].reservation_id(), reservation_id);
    let retry = Box::pin(
        fixture
            .scope
            .claim_recoverable_post_jit_retirement(&candidates[0], 60),
    )
    .await?
    .ok_or(HostError::Journal)?;
    assert_eq!(retry.claim_generation(), 2);
    assert_eq!(retry.reservation_id(), reservation_id);
    assert_retry_preserved(&fixture.journal, reservation_id, &snapshot).await?;
    assert!(
        Box::pin(fixture.scope.recoverable_post_jit_retirements())
            .await?
            .is_empty()
    );
    Ok(())
}

async fn assert_retry_preserved(
    journal: &Journal,
    reservation_id: i64,
    snapshot: &str,
) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT reservation.state,reservation.local_cleanup_proven,state.storage_cleanup_proven,(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims claim WHERE claim.reservation_id=reservation.id),(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=reservation.id AND phase.claim_generation=1 AND phase.phase_index=3 AND phase.phase='storage_absent'),(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=reservation.id AND phase.claim_generation=2 AND phase.phase_index=0 AND phase.phase='claimed'),(SELECT snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims claim WHERE claim.reservation_id=reservation.id AND claim.claim_generation=1),(SELECT snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims claim WHERE claim.reservation_id=reservation.id AND claim.claim_generation=2) FROM capacity_reservations reservation JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id WHERE reservation.id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let local_proof: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let storage_proof: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let claims: i64 = row.get(3).map_err(|_| HostError::Journal)?;
    let prior_phase3: i64 = row.get(4).map_err(|_| HostError::Journal)?;
    let retry_phase0: i64 = row.get(5).map_err(|_| HostError::Journal)?;
    let prior_snapshot: String = row.get(6).map_err(|_| HostError::Journal)?;
    let retry_snapshot: String = row.get(7).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || !matches!(state.as_str(), "reserved" | "uncertain" | "recovering")
        || local_proof != 1
        || storage_proof != 1
        || claims != 2
        || prior_phase3 != 1
        || retry_phase0 != 1
        || prior_snapshot != snapshot
        || retry_snapshot != snapshot
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
