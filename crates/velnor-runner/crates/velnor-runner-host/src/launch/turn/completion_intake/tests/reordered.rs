use super::*;

use serde_json::{Value, json};
use velnor_runner_github::{InnerKind, Poll, parse_poll};

const JOB_ID: &str = "same-request-reordered-job";

#[test]
fn completed_started_assigned_same_request_replay_keeps_terminal_state() -> Result<(), HostError> {
    on_large_test_stack(completed_started_assigned_same_request_replay_keeps_terminal_state_inner())
}

async fn completed_started_assigned_same_request_replay_keeps_terminal_state_inner()
-> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let poll = parse_reordered_batch(&fixture)?;
    assert_parsed_order(&poll, &fixture)?;

    let first_ack = AtomicBool::new(false);
    let first = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &poll,
        || async {
            assert_completed_ack_state(&fixture).await?;
            first_ack.store(true, Ordering::SeqCst);
            Ok::<(), EnsureError>(())
        },
    )
    .await
    .map_err(|_| HostError::Journal)?;
    assert_decision(&first);
    assert!(first_ack.load(Ordering::SeqCst));

    let replay_ack = AtomicBool::new(false);
    let replay = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &poll,
        || async {
            assert_completed_ack_state(&fixture).await?;
            replay_ack.store(true, Ordering::SeqCst);
            Ok::<(), EnsureError>(())
        },
    )
    .await
    .map_err(|_| HostError::Journal)?;
    assert_decision(&replay);
    assert!(replay_ack.load(Ordering::SeqCst));
    Ok(())
}

fn parse_reordered_batch(fixture: &NativeStartedIntakeFixture) -> Result<Poll, HostError> {
    let events = json!([
        wire_event(fixture, "JobCompleted"),
        wire_event(fixture, "JobStarted"),
        wire_event(fixture, "JobAssigned"),
    ])
    .to_string();
    let envelope = json!({
        "messageId": fixture.message_id,
        "messageType": "RunnerScaleSetJobMessages",
        "body": events,
    })
    .to_string();
    parse_poll(200, &envelope).map_err(|_| HostError::Journal)
}

fn wire_event(fixture: &NativeStartedIntakeFixture, message_type: &str) -> Value {
    let source = &fixture.source;
    let mut event = json!({
        "messageType": message_type,
        "runnerRequestId": fixture.request_id,
        "jobId": JOB_ID,
        "ownerName": source.owner_name,
        "repositoryName": source.repository_name,
        "jobWorkflowRef": source.job_workflow_ref,
        "workflowRunId": source.workflow_run_id,
        "eventName": source.event_name,
    });
    if message_type != "JobAssigned" {
        event["runnerId"] = json!(fixture.runner.id);
        event["runnerName"] = json!(fixture.runner.name);
    }
    if message_type == "JobCompleted" {
        event["result"] = json!("Succeeded");
    }
    event
}

fn assert_parsed_order(poll: &Poll, fixture: &NativeStartedIntakeFixture) -> Result<(), HostError> {
    let Poll::Batch(batch) = poll else {
        return Err(HostError::Journal);
    };
    assert_eq!(batch.message_id, fixture.message_id);
    assert_eq!(batch.jobs.len(), 3);
    assert!(matches!(&batch.jobs[0].kind, InnerKind::Completed));
    assert!(matches!(&batch.jobs[1].kind, InnerKind::Started));
    assert!(matches!(&batch.jobs[2].kind, InnerKind::Assigned));
    for job in &batch.jobs[..2] {
        assert_eq!(job.runner_id, Some(fixture.runner.id));
        assert_eq!(
            job.runner_name.as_deref(),
            Some(fixture.runner.name.as_str())
        );
    }
    assert_eq!(batch.jobs[0].result.as_deref(), Some("Succeeded"));
    assert_eq!(batch.jobs[1].result, None);
    assert_eq!(batch.jobs[2].runner_id, None);
    assert_eq!(batch.jobs[2].runner_name, None);
    assert!(batch.jobs.iter().all(|job| {
        job.request_id == Some(fixture.request_id)
            && job.job_id.as_deref() == Some(JOB_ID)
            && job.source == fixture.source
    }));
    Ok(())
}

async fn assert_completed_ack_state(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), EnsureError> {
    let launches = fixture
        .scope
        .due_completed_launches(i64::MAX, 10)
        .await
        .map_err(|_| EnsureError::AdmissionClosed)?;
    if launches.len() != 1 || launches[0].intent.id != fixture.intent_id {
        return Err(EnsureError::AdmissionClosed);
    }
    let intent = fixture
        .scope
        .intent(fixture.intent_id)
        .await
        .map_err(|_| EnsureError::AdmissionClosed)?;
    let occupied = fixture
        .journal
        .occupied_reservation_count()
        .await
        .map_err(|_| EnsureError::AdmissionClosed)?;
    if !intent.runner_completed
        || intent.launch_phase != Some(crate::reconcile::LaunchPhase::RunnerCompleted)
        || occupied != 1
    {
        return Err(EnsureError::AdmissionClosed);
    }
    Ok(())
}

fn assert_decision(decision: &super::super::IntakeDecision) {
    assert!(decision.completion_only);
    assert!(decision.wake_cleanup);
    assert!(!decision.session_delete_candidate);
    assert_eq!(decision.started_request_ids, [] as [i64; 0]);
}
