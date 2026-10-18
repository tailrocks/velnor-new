use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::ConnectionId;
use crate::journal::{AssignedAcquireResult, RequestAcquireState};
use crate::journal::{AssignmentIdentity, AssignmentKey};
use crate::launch::inspect_tests::DockerStub;
use crate::launch::resource_probe::StartPermit;
use crate::worker::Started;
use serde_json::json;
use velnor_runner_github::Method;

use super::super::acquire::{self, AcquireDisposition};
use super::super::runner;
use super::super::validate_scope_binding;
use support::{
    FakeLane, Fixture, REQUEST_ID, SET_ID, assert_runner_name, docker_identity_stub, exchange,
    selected_engine, test_images,
};

#[path = "runner_tests/docker_pair.rs"]
mod docker_pair;
#[path = "runner_tests/support.rs"]
mod support;

#[tokio::test]
async fn acquire_rejects_connection_scale_set_and_request_identity_mismatches_before_http()
-> Result<(), String> {
    let fixture = Fixture::open("scoped-identity-mismatch").await?;
    let mut lane = FakeLane::new([
        Ok(exchange(200, r#"{"count":1,"value":[41]}"#)),
        Ok(exchange(200, r#"{"count":1,"value":[41]}"#)),
        Ok(exchange(200, r#"{"count":1,"value":[41]}"#)),
    ]);

    let other_connection =
        ConnectionId::parse("other-connection").map_err(|error| error.to_string())?;
    let wrong_connection = AssignmentIdentity::new(
        &other_connection,
        SET_ID,
        AssignmentKey::AssignedRequest {
            request_id: REQUEST_ID,
        },
    )
    .map_err(|error| error.to_string())?;
    assert!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &wrong_connection,)
            .await
            .is_err()
    );

    let connection_id = ConnectionId::parse("scoped-adapter").map_err(|error| error.to_string())?;
    let wrong_set = AssignmentIdentity::new(
        &connection_id,
        SET_ID + 1,
        AssignmentKey::AssignedRequest {
            request_id: REQUEST_ID,
        },
    )
    .map_err(|error| error.to_string())?;
    assert!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &wrong_set,)
            .await
            .is_err()
    );

    let wrong_request = AssignmentIdentity::new(
        &connection_id,
        SET_ID,
        AssignmentKey::AssignedRequest {
            request_id: REQUEST_ID + 1,
        },
    )
    .map_err(|error| error.to_string())?;
    assert!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &wrong_request,)
            .await
            .is_err()
    );
    assert!(lane.methods.is_empty());

    assert!(
        validate_scope_binding(&fixture.binding, &fixture.scope, SET_ID + 1)
            .await
            .is_err()
    );
    let mut wrong_connection_binding = fixture.binding.clone();
    wrong_connection_binding.connection_id = other_connection;
    assert!(
        validate_scope_binding(&wrong_connection_binding, &fixture.scope, SET_ID)
            .await
            .is_err()
    );
    let mut wrong_repository = fixture.binding.clone();
    wrong_repository.repository = "owner/other-repository".to_owned();
    assert!(
        validate_scope_binding(&wrong_repository, &fixture.scope, SET_ID)
            .await
            .is_err()
    );
    let mut wrong_policy = fixture.binding.clone();
    wrong_policy.source_policy.events.clear();
    assert!(
        validate_scope_binding(&wrong_policy, &fixture.scope, SET_ID)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn exact_acquire_jit_and_bounded_docker_preflight_use_one_physical_nonce()
-> Result<(), String> {
    let fixture = Fixture::open("scoped-exact").await?;
    let permit = StartPermit::test_fixture();
    let root_digest = permit.docker_root_digest().to_owned();
    let physical = runner::reserve(&fixture.scope, permit)
        .await
        .map_err(|error| error.to_string())?;
    assert_runner_name(&physical.runner_name)?;

    let mut lane = FakeLane::new([
        Ok(exchange(200, r#"{"count":1,"value":[41]}"#)),
        Ok(exchange(
            200,
            &json!({
                "encodedJITConfig": "opaque-jit",
                "runner": {
                    "id": 9001,
                    "name": physical.runner_name,
                    "runnerScaleSetId": SET_ID
                }
            })
            .to_string(),
        )),
    ]);
    assert_eq!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request,)
            .await
            .map_err(|error| error.to_string())?,
        AcquireDisposition::Acquired
    );
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Acquired)
    );

    let images = test_images(&root_digest);
    let selected = selected_engine(&images)?;
    let docker = docker_identity_stub(&images)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let started = runner::start(
        &mut lane,
        "admin-token",
        &fixture.scope,
        &physical,
        &docker.docker,
        &selected,
        &images,
        move |_, _, _, _| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(Started {
                runner_id: "runner-container".to_owned(),
                dind_id: "dind-container".to_owned(),
            }))
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(started.runner_id, "runner-container");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let requests = docker.finish().await?;
    assert_eq!(requests.len(), 8);
    assert!(
        requests
            .iter()
            .filter(|request| request.contains("/info"))
            .count()
            == 4
    );
    assert!(
        requests
            .iter()
            .filter(|request| request.contains("/images/"))
            .count()
            == 4
    );
    assert_eq!(lane.methods, [Method::Post, Method::Post]);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert!(lane.paths[1].contains("/generatejitconfig"));
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert!(
        fixture
            .scope
            .claim_runner_jit(physical.intent_id, &physical.runner_name)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn expiry_after_acquire_keeps_charge_and_sends_no_jit_or_docker_request() -> Result<(), String>
{
    let fixture = Fixture::open("scoped-expiry").await?;
    let permit = StartPermit::test_fixture();
    let root_digest = permit.docker_root_digest().to_owned();
    let stale = permit.stale_observation_witness();
    let mut physical = runner::reserve(&fixture.scope, permit)
        .await
        .map_err(|error| error.to_string())?;
    physical.observation = stale;

    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    assert_eq!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request,)
            .await
            .map_err(|error| error.to_string())?,
        AcquireDisposition::Acquired
    );

    let images = test_images(&root_digest);
    let selected = selected_engine(&images)?;
    let docker = DockerStub::open(Vec::new())?;
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let result = runner::start(
        &mut lane,
        "admin-token",
        &fixture.scope,
        &physical,
        &docker.docker,
        &selected,
        &images,
        move |_, _, _, _| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(Started {
                runner_id: "unexpected-runner".to_owned(),
                dind_id: "unexpected-dind".to_owned(),
            }))
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(lane.methods, [Method::Post]);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(docker.finish().await?.is_empty());
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Acquired)
    );
    assert!(
        fixture
            .scope
            .claim_runner_jit(physical.intent_id, &physical.runner_name)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn exact_no_effect_releases_only_the_unstarted_physical_charge() -> Result<(), String> {
    let fixture = Fixture::open("scoped-no-effect").await?;
    let physical = runner::reserve(&fixture.scope, StartPermit::test_fixture())
        .await
        .map_err(|error| error.to_string())?;
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":0,"value":[]}"#))]);
    assert_eq!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request,)
            .await
            .map_err(|error| error.to_string())?,
        AcquireDisposition::NoEffect
    );
    acquire::release_unstarted(&fixture.scope, physical.reservation_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(0));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::NoEffect)
    );
    assert_eq!(lane.methods, [Method::Post]);
    Ok(())
}

#[tokio::test]
async fn uncertain_acquire_keeps_charge_and_blocks_jit_replay() -> Result<(), String> {
    let fixture = Fixture::open("scoped-uncertain").await?;
    let physical = runner::reserve(&fixture.scope, StartPermit::test_fixture())
        .await
        .map_err(|error| error.to_string())?;
    let mut lane = FakeLane::new([Err(velnor_runner_github::TransportFail::Timeout)]);
    assert!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request,)
            .await
            .is_err()
    );
    assert_eq!(lane.methods, [velnor_runner_github::Method::Post]);
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Uncertain)
    );
    assert!(
        fixture
            .scope
            .claim_runner_jit(physical.intent_id, &physical.runner_name)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn already_acquired_request_never_authorizes_a_new_runner() -> Result<(), String> {
    let fixture = Fixture::open("scoped-already-acquired").await?;
    let physical = runner::reserve(&fixture.scope, StartPermit::test_fixture())
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        fixture
            .scope
            .claim_request_acquire(&fixture.request)
            .await
            .map_err(|error| error.to_string())?
    );
    fixture
        .scope
        .record_request_acquire(&fixture.request, AssignedAcquireResult::AlreadyAcquired)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::AlreadyAcquired)
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert!(
        fixture
            .scope
            .claim_runner_jit(physical.intent_id, &physical.runner_name)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[test]
fn already_acquired_and_uncertain_acquire_results_never_authorize_a_jit() {
    use super::super::acquire::classify;
    use velnor_runner_github::{AcquireOutcome, SessionError};

    let (already, disposition) = classify(&Ok(AcquireOutcome::Noop), support::REQUEST_ID);
    assert_eq!(already, AssignedAcquireResult::AlreadyAcquired);
    assert_eq!(disposition, AcquireDisposition::Uncertain);

    let (uncertain, disposition) = classify(&Err(SessionError::Uncertain), support::REQUEST_ID);
    assert_eq!(uncertain, AssignedAcquireResult::Uncertain);
    assert_eq!(disposition, AcquireDisposition::Uncertain);
}
