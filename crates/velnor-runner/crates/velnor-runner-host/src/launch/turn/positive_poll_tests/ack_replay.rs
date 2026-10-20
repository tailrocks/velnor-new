use super::*;

struct DurableOfferCase {
    scope: crate::journal::ConnectionJournal,
    intent_id: i64,
    allocation: crate::journal::HeaderAllocation,
    request_id: i64,
    message_id: i64,
    polled: Poll,
    offer: crate::launch::drive::DeferredAuthorizedOffer,
    _scratch: crate::launch::turn::scoped_poll_tests::support::Scratch,
}

async fn durable_offer_case() -> Result<DurableOfferCase, String> {
    use crate::config::SourcePolicyMode;
    use crate::journal::{AssignmentKey, SourceAuthorizationResult};
    use crate::launch::{
        drive::DeferredAuthorizedOffer,
        turn::scoped_poll_tests::support::{
            Scratch, authorized_source, repository_connection, session_scope,
        },
    };

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let request_id = 91;
    let message_id = 17;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = crate::launch::steps::source::test_receipt(
        &binding,
        request_id,
        message_id,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    assert_eq!(
        scope
            .record_source_authorization(&identity, &receipt)
            .await
            .map_err(|error| error.to_string())?,
        SourceAuthorizationResult::Recorded
    );
    let polled = available_poll();
    let connection = repository_connection()?;
    let offer = DeferredAuthorizedOffer::from_poll_receipt(
        &connection,
        &scope,
        10_101,
        &polled,
        request_id,
        receipt,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;
    assert!(authorized_deferred_matches(&offer, &polled, &scope, 10_101).await);
    Ok(DurableOfferCase {
        scope,
        intent_id,
        allocation,
        request_id,
        message_id,
        polled,
        offer,
        _scratch: scratch,
    })
}

async fn assert_offer_is_durable(
    scope: &crate::journal::ConnectionJournal,
    request_id: i64,
    message_id: i64,
    offer: &crate::launch::drive::DeferredAuthorizedOffer,
) -> Result<(), String> {
    let replayed = scope
        .authorized_offer(request_id, message_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "authorized offer disappeared after ACK uncertainty".to_owned())?;
    assert_eq!(replayed.source_digest(), offer.offer().source_digest());
    assert_eq!(replayed.source_json(), offer.offer().source_json());
    Ok(())
}

#[tokio::test]
async fn failed_ack_settles_known_get_and_retains_durable_offer() -> Result<(), String> {
    let case = durable_offer_case().await?;
    let attempt = case
        .scope
        .begin_session_poll(case.intent_id, &case.allocation)
        .await
        .map_err(|error| error.to_string())?;
    let settlement = settle_known_response(
        attempt,
        &case.allocation,
        &case.scope,
        10_101,
        "session-scoped-poll",
        case.intent_id,
    )
    .await;
    let outcome = make_outcome(
        intake_decision(),
        Some(PositiveAdmission::Deferred(case.offer)),
        Acknowledgement::Uncertain,
        settlement,
        &case.polled,
    );

    assert_eq!(outcome.acknowledgement, Acknowledgement::Uncertain);
    assert_eq!(outcome.settlement, Settlement::Settled);
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred {
            message_id: case.message_id
        }
    );
    let retained = match outcome.admission {
        Some(PositiveAdmission::Deferred(offer)) => offer,
        _ => return Err("durable authorized offer was lost from the outcome".to_owned()),
    };
    assert_eq!(retained.offer().request_id(), case.request_id);
    assert_eq!(retained.offer().message_id(), case.message_id);
    assert_offer_is_durable(&case.scope, case.request_id, case.message_id, &retained).await
}

fn started_poll(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
) -> Result<Poll, String> {
    let raw_body = serde_json::to_string(&[serde_json::json!({
        "messageType": "JobStarted",
        "runnerRequestId": fixture.request_id,
        "runnerId": fixture.runner.id,
        "runnerName": fixture.runner.name,
        "jobId": "opaque-started-job",
        "ownerName": fixture.source.owner_name,
        "repositoryName": fixture.source.repository_name,
        "jobWorkflowRef": fixture.source.job_workflow_ref,
        "workflowRunId": fixture.source.workflow_run_id,
        "eventName": fixture.source.event_name,
    })])
    .map_err(|error| error.to_string())?;
    Ok(Poll::Batch(ParsedBatch {
        message_id: fixture.message_id,
        jobs: velnor_runner_github::parse_inner_messages(&raw_body)
            .map_err(|error| error.to_string())?,
        raw_body,
        statistics: None,
    }))
}

async fn acknowledge_started_after_record(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    polled: &Poll,
    acknowledged_after_record: &Cell<bool>,
) -> Result<(), String> {
    use crate::launch::turn::completion_intake::intake_and_ack_if_only_scoped;
    use crate::reconcile::LaunchPhase;

    let decision = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        polled,
        || async {
            let intent = fixture
                .scope
                .intent(fixture.intent_id)
                .await
                .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
            acknowledged_after_record.set(intent.launch_phase == Some(LaunchPhase::RunnerStarted));
            Ok::<(), crate::scale_set::EnsureError>(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(decision.completion_only);
    assert_eq!(decision.started_request_ids, [fixture.request_id]);
    assert!(acknowledged_after_record.get());
    Ok(())
}

async fn replay_started(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    polled: &Poll,
) -> Result<(), String> {
    use crate::launch::turn::completion_intake::intake_and_ack_if_only_scoped;
    use crate::reconcile::LaunchPhase;

    let replay = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        polled,
        || async {
            let intent = fixture
                .scope
                .intent(fixture.intent_id)
                .await
                .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?;
            assert_eq!(intent.launch_phase, Some(LaunchPhase::RunnerStarted));
            Ok::<(), crate::scale_set::EnsureError>(())
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(replay.started_request_ids, [fixture.request_id]);
    Ok(())
}

#[tokio::test]
async fn native_started_event_is_recorded_before_ack_and_replay_is_idempotent() -> Result<(), String>
{
    use crate::journal::request_lifecycle::tests::native_started_intake_fixture;
    use crate::reconcile::LaunchPhase;

    let fixture = native_started_intake_fixture()
        .await
        .map_err(|error| error.to_string())?;
    assert_ne!(
        fixture
            .scope
            .intent(fixture.intent_id)
            .await
            .map_err(|error| error.to_string())?
            .launch_phase,
        Some(LaunchPhase::RunnerStarted),
        "the fixture must not record Started before production intake"
    );
    let polled = started_poll(&fixture)?;
    let acknowledged_after_record = Cell::new(false);
    acknowledge_started_after_record(&fixture, &polled, &acknowledged_after_record).await?;
    assert_eq!(
        fixture
            .scope
            .intent(fixture.intent_id)
            .await
            .map_err(|error| error.to_string())?
            .launch_phase,
        Some(LaunchPhase::RunnerStarted)
    );
    replay_started(&fixture, &polled).await
}
