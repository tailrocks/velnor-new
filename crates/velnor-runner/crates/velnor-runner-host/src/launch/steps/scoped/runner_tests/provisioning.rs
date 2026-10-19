use crate::journal::{RequestAcquireState, fixture_provisioning_profile};
use crate::launch::resource_probe::StartPermit;
use velnor_runner_github::Method;

use super::super::super::acquire::{self, AcquireDisposition};
use super::super::super::runner::{self, ReservationOutcome};
use super::support::{FakeLane, Fixture, exchange};

#[tokio::test]
async fn production_reservation_claims_acquire_once_and_replay_sends_none() -> Result<(), String> {
    let fixture = Fixture::open("provisioning-acquire").await?;
    let profile = fixture_provisioning_profile();
    fixture
        .journal
        .configure_storage_profile(profile.clone())
        .await
        .map_err(|error| error.to_string())?;

    let permit = StartPermit::test_fixture();
    let admission = permit
        .storage_provisioning_admission(&profile, permit.engine_id(), permit.docker_root_digest())
        .ok_or_else(|| "production sample builder rejected its matching permit".to_owned())?;
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    let first = runner::reserve_and_claim(&fixture.scope, permit, admission, &fixture.request)
        .await
        .map_err(|error| error.to_string())?;
    let ReservationOutcome::AcquireClaimed(_, claim) = first else {
        return Err("new request did not receive the atomic Acquire claim".to_owned());
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

    let replay_permit = StartPermit::test_fixture_with_generation(&"b".repeat(32));
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
