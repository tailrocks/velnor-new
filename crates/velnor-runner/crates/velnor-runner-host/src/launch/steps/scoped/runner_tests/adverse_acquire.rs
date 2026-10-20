use crate::journal::{AssignmentKey, RequestAcquireState};
use crate::launch::resource_probe::StartPermit;
use crate::scale_set::EnsureError;
use velnor_runner_github::{Method, Poll};

use super::atomic_acquire::{ActiveHostState, drive, prepare_capacity};
use super::support::{
    FakeLane, Fixture, MESSAGE_ID, REQUEST_ID, SET_ID, exchange, selected_engine, test_images,
};

#[path = "adverse_acquire/source_api.rs"]
mod source_api;
use source_api::{SourceApi, trusted_metadata};

#[path = "adverse_acquire/session.rs"]
mod session;

#[path = "adverse_acquire/ack_replay.rs"]
mod ack_replay;

#[tokio::test]
async fn empty_acquire_releases_charge_and_rejects_replay() -> Result<(), String> {
    assert_definite_acquire(
        "empty-acquire",
        exchange(200, r#"{"count":0,"value":[]}"#),
        RequestAcquireState::NoEffect,
    )
    .await
}

#[tokio::test]
async fn forbidden_acquire_releases_charge_and_rejects_replay() -> Result<(), String> {
    assert_definite_acquire(
        "forbidden-acquire",
        exchange(403, "{}"),
        RequestAcquireState::Rejected,
    )
    .await
}

async fn assert_definite_acquire(
    label: &str,
    response: velnor_runner_github::Exchange,
    expected_state: RequestAcquireState,
) -> Result<(), String> {
    let fixture = Fixture::open(label).await?;
    prepare_capacity(&fixture).await?;
    fixture
        .journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await
        .map_err(|error| error.to_string())?;
    let state = ActiveHostState::new(&fixture.binding)?;
    let context = drive(&fixture, &state.path);
    let permit =
        StartPermit::test_fixture_with_available_memory(&"f".repeat(32), 32 * 1024 * 1024 * 1024);
    let expected = selected_engine(&test_images(permit.docker_root_digest()))?;
    let mut poll = crate::launch_harness::available(&[REQUEST_ID]);
    if let Poll::Batch(batch) = &mut poll {
        batch.message_id = MESSAGE_ID;
        batch.jobs[0].source = trusted_metadata();
    }
    let mut lane = FakeLane::new([Ok(response)]);
    let outcome =
        crate::launch::steps::acquire_scoped(&mut lane, &context, &poll, permit, &expected)
            .await
            .map_err(|error| error.to_string())?;
    let crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(receipt) = outcome else {
        return Err("the exact no-acquire receipt is absent".to_owned());
    };
    assert_eq!(receipt.request().request_id(), Some(REQUEST_ID));
    assert_eq!(receipt.state(), expected_state);
    assert!(receipt.reservation_released());
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(lane.paths.len(), 1);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(expected_state)
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        0
    );

    let fixture = fixture.reopen().await?;
    let mut wrong_poll = poll.clone();
    if let Poll::Batch(batch) = &mut wrong_poll {
        batch.jobs[0].source.repository_name = Some("other-repository".to_owned());
    }
    let mut wrong_source_api = SourceApi::default();
    let wrong = crate::launch::authorize_positive_offer_with(
        &fixture.binding,
        &fixture.scope,
        "test-token",
        SET_ID,
        &wrong_poll,
        || async {
            let Poll::Batch(batch) = &wrong_poll else {
                return Err(EnsureError::AdmissionClosed);
            };
            crate::launch::steps::authorize_offer_with_transport(
                Some(&fixture.binding),
                Some(&fixture.scope),
                "test-token",
                batch,
                REQUEST_ID,
                &mut wrong_source_api,
            )
            .await
        },
    )
    .await;
    assert!(matches!(wrong, Err(EnsureError::SourceBlocked { .. })));
    assert_eq!(wrong_source_api.calls, 1);
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(expected_state)
    );

    let mut source_api = SourceApi::default();
    let authorization = crate::launch::authorize_positive_offer_with(
        &fixture.binding,
        &fixture.scope,
        "test-token",
        SET_ID,
        &poll,
        || async {
            let Poll::Batch(batch) = &poll else {
                return Err(EnsureError::AdmissionClosed);
            };
            crate::launch::steps::authorize_offer_with_transport(
                Some(&fixture.binding),
                Some(&fixture.scope),
                "test-token",
                batch,
                REQUEST_ID,
                &mut source_api,
            )
            .await
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    let crate::launch::PositiveOfferAuthorization::TerminalNoAcquire(terminal) = authorization
    else {
        return Err("The verified source has no terminal acquire result.".to_owned());
    };
    assert_eq!(terminal.request().request_id(), Some(REQUEST_ID));
    assert_eq!(terminal.state(), expected_state);
    assert!(terminal.reservation_released());
    assert_eq!(source_api.calls, 1);
    let reopened_state = ActiveHostState::new(&fixture.binding)?;
    let reopened_context = drive(&fixture, &reopened_state.path);
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(expected_state)
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        0
    );
    let mut replay = FakeLane::new([]);
    let replayed = crate::launch::steps::acquire_scoped(
        &mut replay,
        &reopened_context,
        &poll,
        StartPermit::test_fixture_with_available_memory(&"e".repeat(32), 32 * 1024 * 1024 * 1024),
        &expected,
    )
    .await
    .map_err(|error| error.to_string())?;
    let crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(replayed) = replayed else {
        return Err("the stored no-acquire result was not returned".to_owned());
    };
    assert_eq!(replayed.request().request_id(), Some(REQUEST_ID));
    assert_eq!(replayed.state(), expected_state);
    assert!(replayed.reservation_released());
    assert!(replay.methods.is_empty());
    assert!(replay.paths.is_empty());
    Ok(())
}

#[tokio::test]
async fn unusable_acquire_successes_hold_charge_and_block_replay() -> Result<(), String> {
    for (label, response) in [
        ("malformed", exchange(200, "not-json")),
        (
            "foreign-request",
            exchange(200, r#"{"count":1,"value":[42]}"#),
        ),
        ("server-error", exchange(500, "{}")),
    ] {
        assert_unusable_acquire_case(label, response).await?;
    }
    Ok(())
}

async fn assert_unusable_acquire_case(
    label: &str,
    response: velnor_runner_github::Exchange,
) -> Result<(), String> {
    let fixture = Fixture::open(label).await?;
    prepare_capacity(&fixture).await?;
    fixture
        .journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await
        .map_err(|error| error.to_string())?;
    let state = ActiveHostState::new(&fixture.binding)?;
    let context = drive(&fixture, &state.path);
    let permit =
        StartPermit::test_fixture_with_available_memory(&"a".repeat(32), 32 * 1024 * 1024 * 1024);
    let expected = selected_engine(&test_images(permit.docker_root_digest()))?;
    let poll = crate::launch_harness::available(&[REQUEST_ID]);
    let mut lane = FakeLane::new([Ok(response)]);
    assert!(
        crate::launch::steps::acquire_scoped(&mut lane, &context, &poll, permit, &expected)
            .await
            .is_err(),
        "{label}"
    );
    assert_eq!(lane.methods, [Method::Post], "{label}");
    assert_eq!(lane.paths.len(), 1, "{label}");
    assert!(lane.paths[0].ends_with("/acquirejobs"), "{label}");
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Uncertain),
        "{label}"
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1,
        "{label}"
    );
    assert_acquire_replay_stays_held(&fixture, &context, &poll, &expected, label).await
}

async fn assert_acquire_replay_stays_held(
    fixture: &Fixture,
    context: &crate::launch::Drive,
    poll: &velnor_runner_github::Poll,
    expected: &crate::launch::SelectedEngineContext,
    label: &str,
) -> Result<(), String> {
    let mut replay = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    let replayed = crate::launch::steps::acquire_scoped(
        &mut replay,
        context,
        poll,
        StartPermit::test_fixture_with_available_memory(&"b".repeat(32), 32 * 1024 * 1024 * 1024),
        expected,
    )
    .await;
    assert!(matches!(replayed, Err(EnsureError::Uncertain)), "{label}");
    assert!(replay.methods.is_empty(), "{label}");
    let rows = fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1, "{label}");
    let row = &rows[0];
    assert_eq!(
        row.reservation.state,
        crate::journal::ReservationState::Reserved,
        "{label}"
    );
    assert_eq!(row.reservation.cpu_millicores, 2_000, "{label}");
    assert_eq!(row.reservation.memory_bytes, 8_589_934_592, "{label}");
    assert_eq!(row.runner, None, "{label}");
    assert_eq!(row.runner_container_id, None, "{label}");
    assert_eq!(row.dind_container_id, None, "{label}");
    assert_eq!(
        row.intent_state,
        Some(crate::journal::IntentState::Pending),
        "{label}"
    );
    assert_eq!(row.requested_runner_name, None, "{label}");
    let assignment = row
        .ownership
        .assignment()
        .ok_or_else(|| format!("{label}: physical assignment is unknown"))?;
    let requested_name = match assignment.key() {
        AssignmentKey::Remote { subject, .. } => format!("v{subject}"),
        AssignmentKey::AssignedRequest { .. }
        | AssignmentKey::PopulationMessage { .. }
        | AssignmentKey::Session { .. } => {
            return Err(format!("{label}: physical assignment is not a runner"));
        }
    };
    let (generation, digest) = fixture
        .scope
        .test_active_authority_identity()
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        fixture
            .scope
            .claim_runner_jit_for_request(
                row.intent_id,
                row.reservation_id,
                REQUEST_ID,
                &requested_name,
                generation,
                &digest
            )
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "{label}"
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1,
        "{label}"
    );
    Ok(())
}
