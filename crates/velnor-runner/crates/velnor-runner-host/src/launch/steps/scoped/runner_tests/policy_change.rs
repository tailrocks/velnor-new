use crate::journal::ReservationState;
use crate::launch::resource_probe::StartPermit;
use velnor_runner_github::Method;

use super::super::super::acquire::{self, AcquireDisposition};
use super::super::super::runner::{self, ReservationOutcome};
use super::support::{FakeLane, Fixture, configure_provisioning_profile, exchange};

#[tokio::test]
async fn fresh_jit_claim_rejects_a_policy_changed_after_acquire() -> Result<(), String> {
    let fixture = Fixture::open("scoped-policy-change").await?;
    let profile = configure_provisioning_profile(&fixture).await?;
    let permit =
        StartPermit::test_fixture_with_available_memory(&"a".repeat(32), 32 * 1024 * 1024 * 1024);
    let storage = permit
        .storage_provisioning_admission(&profile, permit.engine_id(), permit.docker_root_digest())
        .ok_or_else(|| "The sample does not meet storage admission rules.".to_owned())?;
    let reserved = runner::reserve_and_claim(
        &fixture.scope,
        permit,
        storage,
        &fixture.request,
        &fixture.authority_policy,
    )
    .await
    .map_err(|error| format!("The reservation failed. Error: {error}."))?;
    let ReservationOutcome::AcquireClaimed(physical, acquire_claim) = reserved else {
        return Err(match reserved {
            ReservationOutcome::Existing => "The request already has a reservation.".to_owned(),
            ReservationOutcome::AtCapacity => "The host has no available capacity.".to_owned(),
            ReservationOutcome::ReconcileRequired => "The request needs recovery.".to_owned(),
            ReservationOutcome::AcquireClaimed(_, _) => unreachable!(),
        });
    };
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    assert_eq!(
        acquire::run_claimed(&mut lane, "queue-token", &fixture.scope, acquire_claim)
            .await
            .map_err(|error| format!("The Acquire operation failed. Error: {error}."))?,
        AcquireDisposition::Acquired
    );
    assert_eq!(lane.methods, [Method::Post]);

    let changed = crate::config::HostConfig::test_with_job_authority_policy_generation(
        &fixture.binding,
        fixture.authority_policy.generation() + 1,
    )
    .map_err(|error| error.to_string())?;
    fixture
        .scope
        .install_job_authority_policy(&changed)
        .await
        .map_err(|error| format!("The policy update failed. Error: {error}."))?;
    assert!(
        fixture
            .scope
            .claim_runner_jit_for_request(
                physical.intent_id,
                physical.reservation_id,
                fixture
                    .request
                    .request_id()
                    .ok_or_else(|| "The request ID is missing.".to_owned())?,
                &physical.runner_name,
                physical.authority_generation,
                &physical.authority_digest,
            )
            .await
            .map_err(|error| format!("The JIT claim failed. Error: {error}."))?
            .is_none()
    );
    let reservations = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = reservations
        .iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or_else(|| "The physical reservation is missing.".to_owned())?;
    assert_eq!(row.reservation.state, ReservationState::Reserved);
    assert_eq!(format!("{:?}", row.jit_state), "Absent");
    assert_eq!(lane.methods, [Method::Post]);
    Ok(())
}
