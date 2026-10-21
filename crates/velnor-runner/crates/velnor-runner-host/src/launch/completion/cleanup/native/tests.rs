use crate::{error::HostError, journal::MetadataSettlementResult};

use super::finish_after_settlement;

#[path = "tests/support.rs"]
mod support;
pub(super) use support::{
    capacity_retry_fixture, diagnostics_root, run_native_cleanup,
    run_native_cleanup_with_diagnostics, seed_completion,
};

#[test]
fn settled_metadata_finishes_completion() {
    assert!(matches!(
        finish_after_settlement(MetadataSettlementResult::Settled),
        Ok(())
    ));
}

#[test]
fn capacity_hold_keeps_completion_retryable() {
    assert!(matches!(
        finish_after_settlement(MetadataSettlementResult::HeldAtCapacity),
        Err(super::super::Failure {
            phase: "metadata capacity hold",
            reason: "not_proven",
            error: HostError::Journal,
            stop_requested: false,
        })
    ));
}

#[test]
fn capacity_hold_retry_skips_proven_cleanup_effects() {
    assert_eq!(
        super::CleanupPhases::from_proofs(false, false, false, false),
        super::CleanupPhases {
            remote: true,
            storage: true,
            network: true,
            local: true,
        }
    );
    assert!(finish_after_settlement(MetadataSettlementResult::HeldAtCapacity).is_err());

    assert_eq!(
        super::CleanupPhases::from_proofs(true, true, true, true),
        super::CleanupPhases {
            remote: false,
            storage: false,
            network: false,
            local: false,
        }
    );
}

#[tokio::test]
async fn native_completion_reads_exact_charged_reservation() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let completed_assignment = fixture
        .scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest {
            request_id: fixture.request_id,
        })
        .await?;
    assert_eq!(
        fixture
            .scope
            .record_job_completed(
                &completed_assignment,
                fixture.message_id,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some("opaque-started-job"),
            )
            .await?,
        Some(fixture.intent_id)
    );
    let launches = fixture.scope.due_completed_launches(0, 1).await?;
    assert_eq!(launches.len(), 1);
    let launch = &launches[0];
    assert_eq!(launch.intent.id, fixture.intent_id);
    assert!(launch.intent.worker_volume.is_none());

    let reservation = fixture
        .scope
        .native_completion_reservation(launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id(), fixture.reservation_id);
    assert_eq!(reservation.intent_id(), fixture.intent_id);
    assert_eq!(
        reservation.runner_container_id(),
        fixture.channel.runner_container_id()
    );
    assert_eq!(
        reservation.dind_container_id(),
        fixture.channel.dind_container_id()
    );
    assert!(!reservation.remote_cleanup_proven());
    assert!(!reservation.storage_cleanup_proven());
    assert!(!reservation.local_cleanup_proven());
    Ok(())
}

#[test]
fn native_capacity_retry_does_not_repeat_proven_cleanup_effects() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "native_capacity_retry_does_not_repeat_proven_cleanup_effects",
        || {
            Box::pin(async {
                Box::pin(
                    native_capacity_retry_does_not_repeat_proven_cleanup_effects_on_fixture_stack(),
                )
                .await
                .map_err(|error| error.to_string())
            })
        },
    )
}

async fn native_capacity_retry_does_not_repeat_proven_cleanup_effects_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = capacity_retry_fixture().await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
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

#[test]
fn terminal_replay_requires_the_exact_intent_and_normal_outcome() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "terminal_replay_requires_the_exact_intent_and_normal_outcome",
        || {
            Box::pin(async {
                terminal_replay_requires_the_exact_intent_and_normal_outcome_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn terminal_replay_requires_the_exact_intent_and_normal_outcome_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = capacity_retry_fixture().await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
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

async fn assert_capacity_hold_keeps_attempt(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    launch: &crate::journal::CompletedLaunch,
    claim: crate::journal::CleanupClaim,
    now: i64,
) -> Result<i64, HostError> {
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, claim.generation, now)
            .await?
    );
    let first = run_native_cleanup(fixture, launch, claim).await?;
    assert!(
        matches!(
            first,
            Err(super::super::Failure {
                phase: "metadata capacity hold",
                ..
            })
        ),
        "Native cleanup returned {first:?}."
    );
    let after_first = fixture.server.effects();
    assert_eq!(after_first.observer_runs, 1);
    assert_eq!(after_first.capacity_runs, 1);
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture._base.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .state,
        crate::journal::ReservationState::Reserved
    );
    assert!(!fixture.scope.intent(launch.intent.id).await?.cleanup_proven);
    assert!(
        fixture
            .scope
            .current_physical_attempt(fixture.reservation_id)
            .await?
            .is_some()
    );
    assert!(
        !fixture
            .scope
            .terminal_settlement_matches_at(launch.intent.id, claim.generation)
            .await?
    );
    Ok(claim.generation)
}

async fn assert_retry_settlement_is_idempotent(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    launch: &crate::journal::CompletedLaunch,
    now: i64,
    first_generation: i64,
) -> Result<(), HostError> {
    let after_first = fixture.server.effects();
    assert!(
        fixture
            .scope
            .retry_completion_cleanup_at(launch.intent.id, first_generation, now, now + 1)
            .await?
    );
    let retry_claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now + 1, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, retry_claim.generation, now + 1,)
            .await?
    );
    let second = run_native_cleanup(&fixture, &launch, retry_claim).await?;
    assert!(second.is_ok(), "Native cleanup retry failed: {second:?}.");
    assert_eq!(
        fixture
            .scope
            .completion_cleanup_schedule_at(launch.intent.id)
            .await?,
        Some((0, 0))
    );
    let after_retry = fixture.server.effects();
    assert_eq!(after_retry.observer_runs, after_first.observer_runs);
    assert_eq!(after_retry.container_deletes, after_first.container_deletes);
    assert_eq!(after_retry.volume_deletes, after_first.volume_deletes);
    assert_eq!(after_retry.network_deletes, after_first.network_deletes);
    assert_eq!(after_retry.capacity_runs, after_first.capacity_runs + 1);
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture._base.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .state,
        crate::journal::ReservationState::Released
    );
    assert!(
        !fixture
            .scope
            .record_completion_cleanup_at(
                launch.intent.id,
                retry_claim.generation + 1,
                now + 1_000,
            )
            .await?
    );
    assert!(
        fixture
            .scope
            .record_completion_cleanup_at(launch.intent.id, retry_claim.generation, now + 1_000,)
            .await?
    );
    assert!(
        fixture
            .scope
            .record_completion_cleanup_at(launch.intent.id, retry_claim.generation, now + 1_001,)
            .await?
    );
    assert!(fixture.scope.intent(launch.intent.id).await?.cleanup_proven);
    Ok(())
}
