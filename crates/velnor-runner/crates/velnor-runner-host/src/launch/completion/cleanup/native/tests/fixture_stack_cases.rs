use super::*;

pub(super) async fn native_capacity_retry_does_not_repeat_proven_cleanup_effects_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = capacity_retry_fixture().await?;
    let (launch, now) = seed_completion(&fixture).await?;
    let first_claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let first_generation =
        assert_capacity_hold_keeps_attempt(&fixture, &launch, first_claim, now).await?;
    assert_retry_settlement_is_idempotent(&fixture, &launch, now, first_generation).await
}

pub(super) async fn terminal_replay_requires_the_exact_intent_and_normal_outcome_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = capacity_retry_fixture().await?;
    let (launch, now) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, claim.generation, now)
            .await?
    );
    assert!(matches!(
        run_native_cleanup(&fixture, &launch, claim).await?,
        Err(super::super::Failure {
            phase: "metadata capacity hold",
            ..
        })
    ));

    assert!(
        fixture
            .scope
            .record_completion_cleanup_at(launch.intent.id, claim.generation, now)
            .await?
    );
    assert!(
        !fixture
            .scope
            .terminal_settlement_matches_at(launch.intent.id, claim.generation)
            .await?
    );
    assert!(
        !fixture
            .scope
            .terminal_settlement_matches_at(launch.intent.id + 1, claim.generation)
            .await?
    );
    assert!(
        !fixture
            .scope
            .record_completion_cleanup_at(launch.intent.id, claim.generation, now + 1)
            .await?
    );
    Ok(())
}
