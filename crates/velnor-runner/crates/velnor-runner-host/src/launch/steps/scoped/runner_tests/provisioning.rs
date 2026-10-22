use crate::journal::RequestAcquireState;
use crate::launch::resource_probe::StartPermit;
use velnor_runner_github::Method;

use super::super::super::acquire::{self, AcquireDisposition};
use super::super::super::runner::{self, ReservationOutcome};
use super::support::{FakeLane, Fixture, configure_provisioning_profile, exchange};

#[tokio::test]
async fn production_reservation_claims_acquire_once_and_replay_sends_none() -> Result<(), String> {
    let fixture = Fixture::open("provisioning-acquire").await?;
    let profile = configure_provisioning_profile(&fixture).await?;

    let permit =
        StartPermit::test_fixture_with_available_memory(&"a".repeat(32), 32 * 1024 * 1024 * 1024);
    let admission = permit
        .storage_provisioning_admission(&profile, permit.engine_id(), permit.docker_root_digest())
        .ok_or_else(|| "production sample builder rejected its matching permit".to_owned())?;
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    let first = runner::reserve_and_claim(
        &fixture.scope,
        permit,
        admission,
        &fixture.request,
        &fixture.authority_policy,
    )
    .await
    .map_err(|error| error.to_string())?;
    let ReservationOutcome::AcquireClaimed(_, claim) = first else {
        return Err(match first {
            ReservationOutcome::Existing => "The request already has a reservation.".to_owned(),
            ReservationOutcome::AtCapacity => "The host has no available capacity.".to_owned(),
            ReservationOutcome::ReconcileRequired => "The request needs recovery.".to_owned(),
            ReservationOutcome::AcquireClaimed(_, _) => unreachable!(),
        });
    };
    assert_eq!(
        acquire::run_claimed(&mut lane, "queue-token", &fixture.scope, claim)
            .await
            .map_err(|error| error.to_string())?,
        AcquireDisposition::Acquired
    );
    assert_eq!(lane.methods, [Method::Post]);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Acquired)
    );

    let replay_permit =
        StartPermit::test_fixture_with_available_memory(&"b".repeat(32), 32 * 1024 * 1024 * 1024);
    let replay_admission = replay_permit
        .storage_provisioning_admission(
            &profile,
            replay_permit.engine_id(),
            replay_permit.docker_root_digest(),
        )
        .ok_or_else(|| "replay sample builder rejected its matching permit".to_owned())?;
    let replay = runner::reserve_and_claim(
        &fixture.scope,
        replay_permit,
        replay_admission,
        &fixture.request,
        &fixture.authority_policy,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(matches!(replay, ReservationOutcome::Existing));
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    Ok(())
}
