use crate::error::HostError;
use crate::journal::request_lifecycle::tests::native_started_intake_fixture;

#[tokio::test]
async fn typed_claim_persists_exact_attempt_and_initial_progress() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    super::test_support::recover_fixture(&fixture).await?;
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;

    assert_eq!(claim.reservation_id(), attempt.reservation_id);
    assert_eq!(claim.generation(), attempt.generation);
    assert_eq!(claim.claim_generation(), 1);
    assert_eq!(claim.runner_id(), fixture.runner.id);
    assert_eq!(claim.runner_name(), fixture.runner.name);

    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT claim.snapshot_sha256,claim.lease_until,progress.phase,progress.evidence_sha256 FROM physical_attempt_post_jit_retirement_claims claim JOIN physical_attempt_post_jit_retirement_progress progress ON progress.reservation_id=claim.reservation_id AND progress.claim_generation=claim.claim_generation WHERE claim.reservation_id=?1 AND claim.claim_generation=1 AND progress.phase_index=0",
            [fixture.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let snapshot: String = row.get(0).map_err(|_| HostError::Journal)?;
    let lease_until: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let phase: String = row.get(2).map_err(|_| HostError::Journal)?;
    let evidence: String = row.get(3).map_err(|_| HostError::Journal)?;
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    assert_eq!(snapshot.len(), 64);
    assert!(snapshot.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(lease_until >= claim.lease_until());
    assert_eq!(phase, "claimed");
    assert_eq!(evidence, snapshot);

    Ok(())
}

#[tokio::test]
async fn typed_claim_rejects_invalid_lease_and_foreign_scope() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    assert!(
        fixture
            .scope
            .claim_post_jit_retirement(fixture.reservation_id, 0)
            .await
            .is_err()
    );
    super::test_support::recover_fixture(&fixture).await?;

    let other_binding =
        crate::journal::request_lifecycle::tests::helpers::binding("post-jit-foreign-scope", 202)
            .map_err(|_| HostError::Identity)?;
    let other_scope = crate::journal::request_lifecycle::tests::helpers::created_scope(
        &fixture.journal,
        &other_binding,
        fixture.channel.scale_set_id() + 1,
    )
    .await?;
    assert!(
        other_scope
            .claim_post_jit_retirement(fixture.reservation_id, 60)
            .await?
            .is_none()
    );
    assert!(
        fixture
            .scope
            .claim_post_jit_retirement(fixture.reservation_id, 60)
            .await?
            .is_some()
    );

    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            [fixture.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(count, 1);
    Ok(())
}

#[tokio::test]
async fn live_claim_replays_and_expired_lease_uses_next_generation() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    super::test_support::recover_fixture(&fixture).await?;
    let first = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .claim_post_jit_retirement(fixture.reservation_id, 60)
            .await?
            .is_none()
    );

    tokio::time::sleep(std::time::Duration::from_secs(11)).await;
    let retry = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(retry.claim_generation(), first.claim_generation() + 1);
    assert_eq!(retry.reservation_id(), first.reservation_id());
    assert_eq!(retry.generation(), first.generation());

    let mut rows = fixture
        .journal
        .connection()
        .await?
        .query(
            "SELECT claim_generation,snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 ORDER BY claim_generation",
            [fixture.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let original = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let first_generation: i64 = original.get(0).map_err(|_| HostError::Journal)?;
    let first_snapshot: String = original.get(1).map_err(|_| HostError::Journal)?;
    let second = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let second_generation: i64 = second.get(0).map_err(|_| HostError::Journal)?;
    let second_snapshot: String = second.get(1).map_err(|_| HostError::Journal)?;
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    assert_eq!(first_generation, 1);
    assert_eq!(second_generation, 2);
    assert_eq!(first_snapshot, second_snapshot);
    Ok(())
}

#[tokio::test]
async fn event_disposition_binds_actual_request_to_claimed_runner() -> Result<(), HostError> {
    const ACTUAL_REQUEST: i64 = 9_801;
    let fixture = native_started_intake_fixture().await?;
    crate::journal::request_lifecycle::tests::helpers::record_acquired(
        &fixture.scope,
        ACTUAL_REQUEST,
    )
    .await?;
    super::test_support::recover_fixture(&fixture).await?;
    let before_claim = crate::journal::connection::storage::read_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::post_jit_retirement::disposition_for_event_in_transaction(
                connection,
                &fixture.scope,
                fixture.channel.scale_set_id(),
                ACTUAL_REQUEST,
                fixture.runner.id,
                &fixture.runner.name,
            )
            .await
        },
    )
    .await?;
    assert!(before_claim.is_none());

    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let claimed = crate::journal::connection::storage::read_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::post_jit_retirement::disposition_for_event_in_transaction(
                connection,
                &fixture.scope,
                fixture.channel.scale_set_id(),
                ACTUAL_REQUEST,
                fixture.runner.id,
                &fixture.runner.name,
            )
            .await
        },
    )
    .await?
    .ok_or(HostError::Journal)?;
    assert_eq!(claimed.actual_request_id, ACTUAL_REQUEST);
    assert_eq!(claimed.witness_request_id, attempt.witness_request_id);
    assert_ne!(claimed.actual_request_id, claimed.witness_request_id);
    assert_eq!(claimed.intent_id, attempt.intent_id);
    assert!(matches!(
        claimed.status,
        super::disposition::EventRetirementStatus::Claimed
    ));

    let wrong_runner = crate::journal::connection::storage::read_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::post_jit_retirement::disposition_for_event_in_transaction(
                connection,
                &fixture.scope,
                fixture.channel.scale_set_id(),
                ACTUAL_REQUEST,
                fixture.runner.id + 1,
                &fixture.runner.name,
            )
            .await
        },
    )
    .await?;
    assert!(wrong_runner.is_none());

    let wrong_set = crate::journal::connection::storage::read_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::post_jit_retirement::disposition_for_event_in_transaction(
                connection,
                &fixture.scope,
                fixture.channel.scale_set_id() + 1,
                ACTUAL_REQUEST,
                fixture.runner.id,
                &fixture.runner.name,
            )
            .await
        },
    )
    .await?;
    assert!(wrong_set.is_none());

    let other_binding = crate::journal::request_lifecycle::tests::helpers::binding(
        "post-jit-disposition-other-binding",
        303,
    )
    .map_err(|_| HostError::Identity)?;
    let other_scope = crate::journal::request_lifecycle::tests::helpers::created_scope(
        &fixture.journal,
        &other_binding,
        fixture.channel.scale_set_id(),
    )
    .await?;
    let wrong_binding = crate::journal::connection::storage::read_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::post_jit_retirement::disposition_for_event_in_transaction(
                connection,
                &other_scope,
                fixture.channel.scale_set_id(),
                ACTUAL_REQUEST,
                fixture.runner.id,
                &fixture.runner.name,
            )
            .await
        },
    )
    .await?;
    assert!(wrong_binding.is_none());
    Ok(())
}
