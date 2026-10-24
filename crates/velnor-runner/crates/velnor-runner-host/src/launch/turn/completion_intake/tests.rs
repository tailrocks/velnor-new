use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
};

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture;
use crate::journal::request_lifecycle::tests::post_jit_retirement::late_events;
use crate::launch::turn::completion_intake::intake_and_ack_if_only_scoped;
use crate::scale_set::EnsureError;

#[test]
fn late_lifecycle_batch_acks_without_resuming_or_scheduling_cleanup() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "late-lifecycle-batch-intake",
        || {
            Box::pin(async {
                late_lifecycle_batch_acks_without_resuming_or_scheduling_cleanup_inner()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn late_lifecycle_batch_acks_without_resuming_or_scheduling_cleanup_inner()
-> Result<(), HostError> {
    let fixture = late_events::claimed_native_fixture().await?;
    let poll = lifecycle_batch(&fixture);
    let acknowledged = AtomicBool::new(false);
    let decision = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &poll,
        || async {
            late_events::assert_historical_intake_state(&fixture)
                .await
                .map_err(|_| EnsureError::AdmissionClosed)?;
            acknowledged.store(true, Ordering::SeqCst);
            Ok::<(), EnsureError>(())
        },
    )
    .await
    .map_err(|_| HostError::Journal)?;

    assert!(acknowledged.load(Ordering::SeqCst));
    assert!(decision.completion_only);
    assert!(!decision.wake_cleanup);
    assert_eq!(decision.started_request_ids, [] as [i64; 0]);
    late_events::assert_historical_intake_state(&fixture).await
}

#[test]
fn late_lifecycle_ack_failure_replays_after_journal_reopen() -> Result<(), HostError> {
    on_large_test_stack(late_lifecycle_ack_failure_replays_after_journal_reopen_inner())
}

#[test]
fn scoped_intake_rejects_invalid_batch_bounds_before_ack_or_inbox_write() -> Result<(), HostError> {
    on_large_test_stack(
        scoped_intake_rejects_invalid_batch_bounds_before_ack_or_inbox_write_inner(),
    )
}

async fn scoped_intake_rejects_invalid_batch_bounds_before_ack_or_inbox_write_inner()
-> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let completed = || {
        event_from(
            fixture.request_id,
            &fixture.runner,
            &fixture.source,
            InnerKind::Completed,
        )
    };
    let valid_body = r#"[{"messageType":"JobCompleted"}]"#;
    let mut invalid_message = lifecycle_batch(&fixture);
    let Poll::Batch(batch) = &mut invalid_message else {
        return Err(HostError::Journal);
    };
    batch.message_id = -1;
    batch.raw_body = valid_body.to_owned();
    batch.jobs = vec![completed()];
    assert_rejected_without_ack_or_inbox(&fixture, &invalid_message).await?;

    let oversized_body = "x".repeat(crate::journal::MAX_COMPLETION_BODY_BYTES + 1);
    let oversized = Poll::Batch(ParsedBatch {
        message_id: 9_203,
        raw_body: oversized_body,
        statistics: None,
        jobs: vec![completed()],
    });
    assert_rejected_without_ack_or_inbox(&fixture, &oversized).await?;

    let excessive_jobs = Poll::Batch(ParsedBatch {
        message_id: 9_204,
        raw_body: "[]".to_owned(),
        statistics: None,
        jobs: std::iter::repeat_with(completed)
            .take(velnor_runner_github::MAX_POLL_MESSAGES + 1)
            .collect(),
    });
    assert_rejected_without_ack_or_inbox(&fixture, &excessive_jobs).await
}

#[test]
fn duplicate_active_completion_replay_keeps_one_cleanup_row() -> Result<(), HostError> {
    on_large_test_stack(duplicate_active_completion_replay_keeps_one_cleanup_row_inner())
}

async fn duplicate_active_completion_replay_keeps_one_cleanup_row_inner() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let poll = lifecycle_batch(&fixture);
    let first = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &poll,
        || async { Ok::<(), EnsureError>(()) },
    )
    .await
    .map_err(|_| HostError::Journal)?;
    assert!(first.completion_only);
    assert!(first.wake_cleanup);
    assert_eq!(
        fixture
            .scope
            .due_completed_launches(i64::MAX, 10)
            .await?
            .len(),
        1
    );

    let replay = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &poll,
        || async { Ok::<(), EnsureError>(()) },
    )
    .await
    .map_err(|_| HostError::Journal)?;
    assert!(replay.completion_only);
    assert!(replay.wake_cleanup);
    assert_eq!(
        fixture
            .scope
            .due_completed_launches(i64::MAX, 10)
            .await?
            .len(),
        1
    );
    Ok(())
}

async fn assert_rejected_without_ack_or_inbox(
    fixture: &NativeStartedIntakeFixture,
    poll: &Poll,
) -> Result<(), HostError> {
    let acknowledged = AtomicBool::new(false);
    assert!(
        intake_and_ack_if_only_scoped(
            &fixture.scope,
            fixture.runner.runner_scale_set_id,
            poll,
            || async {
                acknowledged.store(true, Ordering::SeqCst);
                Ok::<(), EnsureError>(())
            },
        )
        .await
        .is_err()
    );
    assert!(!acknowledged.load(Ordering::SeqCst));
    assert_eq!(
        fixture
            .scope
            .pending_completion_inbox(i64::MAX, crate::journal::MAX_COMPLETION_INBOX_SCAN)
            .await?,
        [] as [crate::journal::ConnectionCompletionInboxEntry; 0]
    );
    Ok(())
}

async fn late_lifecycle_ack_failure_replays_after_journal_reopen_inner() -> Result<(), HostError> {
    let fixture = late_events::claimed_native_fixture().await?;
    let poll = lifecycle_batch(&fixture);
    let failed = intake_and_ack_if_only_scoped(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        &poll,
        || async { Err::<(), EnsureError>(EnsureError::AdmissionClosed) },
    )
    .await;
    if failed.is_ok() {
        return Err(HostError::Journal);
    }
    late_events::assert_historical_intake_state(&fixture).await?;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)?;
    let database_path = fixture.database_path.clone();
    let request_id = fixture.request_id;
    let intent_id = fixture.intent_id;
    let reservation_id = fixture.reservation_id;
    let set_id = fixture.runner.runner_scale_set_id;
    let runner = fixture.runner.clone();
    let source = fixture.source.clone();
    let current_before = fixture
        .scope
        .current_physical_attempt(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if current_before.intent_id != intent_id {
        return Err(HostError::Journal);
    }
    let NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let reopened = crate::journal::Journal::open(&database_path).await?;
    let scope = reopened.connection_scope(&binding).await?;
    let poll = lifecycle_batch_from(request_id, &runner, &source);
    let acknowledged = AtomicBool::new(false);
    let decision = intake_and_ack_if_only_scoped(&scope, set_id, &poll, || async {
        late_events::assert_historical_scope_state(&scope, set_id, request_id, intent_id)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        acknowledged.store(true, Ordering::SeqCst);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|_| HostError::Journal)?;
    if !acknowledged.load(Ordering::SeqCst)
        || !decision.completion_only
        || decision.wake_cleanup
        || !decision.started_request_ids.is_empty()
    {
        return Err(HostError::Journal);
    }
    late_events::assert_historical_scope_state(&scope, set_id, request_id, intent_id).await?;
    let current_after = scope
        .current_physical_attempt(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if current_after.intent_id != current_before.intent_id
        || current_after.generation != current_before.generation
    {
        return Err(HostError::Journal);
    }
    let _keep_scratch_until_reopen_checks_finish = scratch;
    Ok(())
}

fn lifecycle_batch(fixture: &NativeStartedIntakeFixture) -> Poll {
    lifecycle_batch_from(fixture.request_id, &fixture.runner, &fixture.source)
}

fn lifecycle_batch_from(
    request_id: i64,
    runner: &velnor_runner_github::RunnerReference,
    source: &velnor_runner_github::SourceMetadata,
) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 9_201,
        raw_body: r#"[{"messageType":"JobStarted"},{"messageType":"JobCompleted"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            event_from(request_id, runner, source, InnerKind::Started),
            event_from(request_id, runner, source, InnerKind::Completed),
        ],
    })
}

fn event_from(
    request_id: i64,
    runner: &velnor_runner_github::RunnerReference,
    source: &velnor_runner_github::SourceMetadata,
    kind: InnerKind,
) -> InnerJob {
    let completed = kind == InnerKind::Completed;
    InnerJob {
        kind,
        request_id: Some(request_id),
        runner_id: Some(runner.id),
        runner_name: Some(runner.name.clone()),
        result: completed.then(|| "Succeeded".to_owned()),
        job_id: Some("late-intake-job".to_owned()),
        labels: Vec::new(),
        fields: Vec::new(),
        source: source.clone(),
    }
}

fn on_large_test_stack(
    future: impl Future<Output = Result<(), HostError>> + Send + 'static,
) -> Result<(), HostError> {
    std::thread::Builder::new()
        .stack_size(8_388_608)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| HostError::Journal)?;
            runtime.block_on(future)
        })
        .map_err(|_| HostError::Journal)?
        .join()
        .map_err(|_| HostError::Journal)?
}
