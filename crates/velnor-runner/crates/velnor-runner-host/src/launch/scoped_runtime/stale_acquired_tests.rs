#[path = "stale_acquired_tests/support.rs"]
mod support;

use support::{Fixture, selected_engine, test_images};
use velnor_runner_github::Method;

use crate::{
    error::HostError,
    journal::{RequestAcquireState, ReservationState},
    launch::{
        bootstrap::adapter::prepare_acquired, inspect_tests::DockerStub,
        steps::ScopedProvisioningOutcome,
    },
};

#[tokio::test]
async fn stale_acquired_witness_is_rejected_before_jit_or_docker_effects() -> Result<(), String> {
    Box::pin(assert_stale_acquired_path()).await
}

async fn assert_stale_acquired_path() -> Result<(), String> {
    let (fixture, mut outcome, lane) = Fixture::acquire("stale").await?;
    let ScopedProvisioningOutcome::Acquired(acquired) = &mut outcome else {
        return Err("The request did not retain its acquired proof.".to_owned());
    };
    let reservation_id = acquired.reservation_id();
    let intent_id = acquired.intent_id();
    let request_id = acquired
        .request()
        .request_id()
        .ok_or_else(|| "The acquired request ID is absent.".to_owned())?;
    let charge = acquired.charge();
    acquired.mark_observation_stale_for_test();

    let images = test_images();
    let engine = selected_engine(&images)?;
    let crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) = &outcome else {
        return Err("The result does not contain an acquired request.".to_owned());
    };
    assert!(!acquired.observation().is_fresh_for_admission(
        engine.engine_id(),
        engine.docker_root_digest(),
        acquired.sample_generation_id(),
    ));
    let docker = DockerStub::open(Vec::new()).map_err(|error| error.clone())?;
    let result = prepare_acquired(outcome, &docker.docker, &fixture.scope, &engine, &images).await;
    if !matches!(result, Err(HostError::Identity)) {
        return Err(format!(
            "The stale witness failed at the wrong boundary: {:?}.",
            result.as_ref().err()
        ));
    }
    assert_eq!(
        docker
            .finish_observed()
            .await
            .map_err(|error| error.clone())?,
        Vec::<String>::new()
    );
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(
        lane.paths,
        ["_apis/runtime/runnerscalesets/501/acquirejobs"]
    );
    assert_acquired_charge(&fixture, reservation_id, intent_id, request_id, charge).await
}

async fn assert_acquired_charge(
    fixture: &Fixture,
    reservation_id: i64,
    intent_id: i64,
    request_id: i64,
    charge: (u64, u64),
) -> Result<(), String> {
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert_eq!(
        fixture.scope.request_acquire_state(&fixture.request).await,
        Ok(Some(RequestAcquireState::Acquired))
    );
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or_else(|| "The acquired physical reservation is absent.".to_owned())?;
    assert_eq!(row.reservation.state, ReservationState::Reserved);
    assert_eq!(row.intent_id, intent_id);
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Absent
    );
    assert!(row.runner.is_none());
    assert!(row.requested_runner_name.is_none());
    assert!(row.runner_request_id.is_none());
    assert!(row.runner_container_id.is_none());
    assert!(row.dind_container_id.is_none());
    assert_eq!(
        (row.reservation.cpu_millicores, row.reservation.memory_bytes),
        charge
    );
    assert_eq!(
        row.acquired_request
            .as_ref()
            .and_then(crate::journal::AssignmentIdentity::request_id),
        Some(request_id)
    );
    Ok(())
}
