use std::cell::Cell;

use crate::journal::RequestAcquireState;
use crate::launch::resource_probe::StartPermit;
use crate::launch::steps::KnownNoAcquire;
use crate::launch::{
    Acknowledgement, FinishContext, IntakeDecision, PositiveAdmission, Settlement,
    finish_positive_poll_with_ack,
};
use crate::scale_set::EnsureError;
use velnor_runner_github::Poll;

use super::super::atomic_acquire::{ActiveHostState, drive, prepare_capacity};
use super::super::support::{
    FakeLane, Fixture, MESSAGE_ID, REQUEST_ID, SET_ID, exchange, selected_engine, test_images,
};
use super::{
    session,
    source_api::{SourceApi, trusted_metadata},
};

#[tokio::test]
async fn no_effect_ack_loss_reopens_and_replays_before_admission() -> Result<(), String> {
    let case = Box::pin(prepare_case(
        "ack-replay",
        exchange(200, r#"{"count":0,"value":[]}"#),
        RequestAcquireState::NoEffect,
    ))
    .await?;
    let first = finish_ack(
        &case.fixture,
        &case.session,
        case.session_intent,
        &case.allocation,
        &case.poll,
        case.receipt,
        Acknowledgement::Uncertain,
    )
    .await?;
    assert_eq!(first.0.acknowledgement, Acknowledgement::Uncertain);
    assert_eq!(first.0.settlement, Settlement::Settled);
    assert_eq!(
        first.0.cursor_disposition,
        crate::launch::CursorDisposition::Deferred {
            message_id: MESSAGE_ID
        }
    );
    assert_eq!(first.1, 1);

    let fixture = case.fixture.reopen().await?;
    let queue_session = session::session()?;
    let allocation = session::reopened_allocation(&fixture.scope, case.session_intent).await?;
    let mut replay_source = SourceApi::default();
    let replay = authorize_terminal(&fixture, &case.poll, &mut replay_source).await?;
    assert_eq!(replay_source.calls, 1);
    assert_eq!(replay.state(), RequestAcquireState::NoEffect);
    assert!(replay.reservation_released());
    let second = finish_ack(
        &fixture,
        &queue_session,
        case.session_intent,
        &allocation,
        &case.poll,
        replay,
        Acknowledgement::Acknowledged,
    )
    .await?;
    assert_eq!(second.0.acknowledgement, Acknowledgement::Acknowledged);
    assert_eq!(second.0.settlement, Settlement::Settled);
    assert_eq!(
        second.0.cursor_disposition,
        crate::launch::CursorDisposition::ResetToHead
    );
    assert_eq!(second.1, 1);
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::NoEffect)
    );
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        0
    );
    assert_eq!(
        case.acquire_calls, 1,
        "replay must not send another AcquireJobs request"
    );
    Ok(())
}

#[tokio::test]
async fn rejected_acquire_keeps_the_queue_message_unacknowledged() -> Result<(), String> {
    let case = Box::pin(prepare_case(
        "rejected-replay",
        exchange(403, "{}"),
        RequestAcquireState::Rejected,
    ))
    .await?;
    let first = finish_ack(
        &case.fixture,
        &case.session,
        case.session_intent,
        &case.allocation,
        &case.poll,
        case.receipt,
        Acknowledgement::Acknowledged,
    )
    .await?;
    assert_eq!(first.0.acknowledgement, Acknowledgement::Deferred);
    assert_eq!(first.0.settlement, Settlement::Settled);
    assert_eq!(
        first.0.cursor_disposition,
        crate::launch::CursorDisposition::Deferred {
            message_id: MESSAGE_ID
        }
    );
    assert_eq!(first.1, 0);

    let fixture = case.fixture.reopen().await?;
    let queue_session = session::session()?;
    let allocation = session::reopened_allocation(&fixture.scope, case.session_intent).await?;
    let mut source = SourceApi::default();
    let receipt = authorize_terminal(&fixture, &case.poll, &mut source).await?;
    assert_eq!(receipt.state(), RequestAcquireState::Rejected);
    let replay = finish_ack(
        &fixture,
        &queue_session,
        case.session_intent,
        &allocation,
        &case.poll,
        receipt,
        Acknowledgement::Acknowledged,
    )
    .await?;
    assert_eq!(replay.0.acknowledgement, Acknowledgement::Deferred);
    assert_eq!(
        replay.0.cursor_disposition,
        crate::launch::CursorDisposition::Deferred {
            message_id: MESSAGE_ID
        }
    );
    assert_eq!(replay.1, 0);
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Rejected)
    );
    assert_eq!(case.acquire_calls, 1);
    Ok(())
}

struct ReplayCase {
    fixture: Fixture,
    session: velnor_runner_github::QueueSession,
    session_intent: i64,
    allocation: crate::journal::HeaderAllocation,
    poll: Poll,
    receipt: KnownNoAcquire,
    acquire_calls: usize,
}

async fn prepare_case(
    label: &str,
    response: velnor_runner_github::Exchange,
    expected_state: RequestAcquireState,
) -> Result<ReplayCase, String> {
    let fixture = Fixture::open(label).await?;
    prepare_capacity(&fixture).await?;
    fixture
        .journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await
        .map_err(|error| error.to_string())?;
    let (session, session_intent, allocation) = session::prepare(&fixture).await?;
    let state = ActiveHostState::new(&fixture.binding)?;
    let context = drive(&fixture, &state.path);
    let permit =
        StartPermit::test_fixture_with_available_memory(&"f".repeat(32), 32 * 1024 * 1024 * 1024);
    let expected = selected_engine(&test_images(permit.docker_root_digest()))?;
    let poll = available_poll();
    let mut lane = FakeLane::new([Ok(response)]);
    let result =
        crate::launch::steps::acquire_scoped(&mut lane, &context, &poll, permit, &expected)
            .await
            .map_err(|error| error.to_string())?;
    let crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(receipt) = result else {
        return Err("The empty acquire result is absent.".to_owned());
    };
    if receipt.state() != expected_state || !receipt.reservation_released() {
        return Err("The response has an unexpected reservation result.".to_owned());
    }
    let mut source = SourceApi::default();
    let _terminal = authorize_terminal(&fixture, &poll, &mut source).await?;
    if source.calls != 1 || lane.methods != [velnor_runner_github::Method::Post] {
        return Err("The first acquire or source check count is wrong.".to_owned());
    }
    Ok(ReplayCase {
        fixture,
        session,
        session_intent,
        allocation,
        poll,
        receipt,
        acquire_calls: lane.methods.len(),
    })
}

async fn finish_ack(
    fixture: &Fixture,
    session: &velnor_runner_github::QueueSession,
    session_intent: i64,
    allocation: &crate::journal::HeaderAllocation,
    poll: &Poll,
    receipt: KnownNoAcquire,
    result: Acknowledgement,
) -> Result<(crate::launch::PositivePollOutcome, usize), String> {
    let attempt = fixture
        .scope
        .begin_session_poll(session_intent, allocation)
        .await
        .map_err(|error| error.to_string())?;
    let mut link = crate::listen::inert_link_for_offline_test();
    let admin_base = link.base().to_owned();
    let calls = Cell::new(0);
    let outcome = finish_positive_poll_with_ack(
        FinishContext {
            link: &mut link,
            session,
            scoped: &fixture.scope,
            session_intent_id: session_intent,
            set_id: SET_ID,
            allocation,
            cursor: 0,
            attempt,
            polled: poll.clone(),
            path: "messages".to_owned(),
            queue: None,
            admin_base,
        },
        intake_decision(),
        Some(PositiveAdmission::KnownNoAcquire(receipt)),
        async |_, _, _, _, _, _| {
            calls.set(calls.get() + 1);
            result
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok((outcome, calls.get()))
}

async fn authorize_terminal(
    fixture: &Fixture,
    poll: &Poll,
    api: &mut SourceApi,
) -> Result<KnownNoAcquire, String> {
    let authorization = crate::launch::authorize_positive_offer_with(
        &fixture.binding,
        &fixture.scope,
        "test-token",
        SET_ID,
        poll,
        || async {
            let Poll::Batch(batch) = poll else {
                return Err(EnsureError::AdmissionClosed);
            };
            crate::launch::steps::authorize_offer_with_transport(
                Some(&fixture.binding),
                Some(&fixture.scope),
                "test-token",
                batch,
                REQUEST_ID,
                api,
            )
            .await
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    match authorization {
        crate::launch::PositiveOfferAuthorization::TerminalNoAcquire(receipt) => Ok(receipt),
        crate::launch::PositiveOfferAuthorization::Deferred(_) => {
            Err("The terminal no-acquire result was not replayed.".to_owned())
        }
    }
}

fn available_poll() -> Poll {
    let mut poll = crate::launch_harness::available(&[REQUEST_ID]);
    if let Poll::Batch(batch) = &mut poll {
        batch.message_id = MESSAGE_ID;
        batch.jobs[0].source = trusted_metadata();
    }
    poll
}

fn intake_decision() -> IntakeDecision {
    IntakeDecision {
        completion_only: false,
        wake_cleanup: false,
        session_delete_candidate: false,
        started_request_ids: Vec::new(),
    }
}
