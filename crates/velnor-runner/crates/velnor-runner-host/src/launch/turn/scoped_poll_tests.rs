use std::cell::Cell;
use std::path::PathBuf;

use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};
use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, SourceMetadata};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, ConnectionJournal,
    CreatedScaleSetResponse, HeaderAllocation, Journal, ScaleSetCreateRequest,
};
use crate::scale_set::EnsureError;

use super::completion_intake::{IntakeDecision, intake_and_ack_if_only_scoped};
use super::finish_scoped_poll_response;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-poll-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

async fn session_scope(
    scratch: &Scratch,
) -> Result<(ConnectionJournal, i64, HeaderAllocation), String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("test-engine")
        .await
        .map_err(|error| error.to_string())?;
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    let binding = RepositoryConnection::try_new(
        ConnectionId::parse("scoped-poll-test").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-poll-test")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    let create = ScaleSetCreateRequest::new(&binding, 7).map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_scale_set_create(&create)
        .await
        .map_err(|error| error.to_string())?;
    let create_claim = scope
        .claim_scale_set_create(&prepared)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Scale Set create claim missing".to_owned())?;
    let created = CreatedScaleSetResponse::verified(
        &create,
        10_101,
        "integration",
        binding.scale_set.labels(),
        true,
    )
    .map_err(|error| error.to_string())?;
    scope
        .record_scale_set_created(create_claim, created)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "session create claim missing".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-scoped-poll", 0)
        .await
        .map_err(|error| error.to_string())?;
    Ok((scope, bound.intent_id(), bound.allocation().clone()))
}

async fn record_acquired_request(
    scope: &ConnectionJournal,
    request_id: i64,
    message_id: i64,
) -> Result<crate::journal::AssignmentIdentity, String> {
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
    scope
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    if !scope
        .claim_request_acquire(&identity)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("request acquire claim missing".to_owned());
    }
    scope
        .record_request_acquire(&identity, AssignedAcquireResult::Returned(vec![request_id]))
        .await
        .map_err(|error| error.to_string())?;
    Ok(identity)
}

fn authorized_source() -> VerifiedWorkflowSource {
    let workflow_ref = "owner/repository/.github/workflows/build.yml@refs/heads/main".to_owned();
    VerifiedWorkflowSource {
        repository_id: 101,
        owner_name: "owner".to_owned(),
        repository_name: "repository".to_owned(),
        workflow_run_id: 10_088,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: workflow_ref.clone(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: workflow_ref,
            owner_name: "owner".to_owned(),
            repository_name: "repository".to_owned(),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

#[tokio::test]
async fn handled_zero_header_response_settles_exact_poll_attempt() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(attempt.header_value(), 0);

    let wake = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        attempt,
        async {
            Ok(IntakeDecision {
                completion_only: true,
                wake_cleanup: true,
                session_delete_candidate: true,
            })
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(wake.wake_cleanup);
    assert!(wake.session_delete_candidate);

    let replay = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(replay.header_value(), 0);
    Ok(())
}

#[tokio::test]
async fn failed_response_handling_holds_the_header_poll() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;

    let result = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        attempt,
        async { Err(EnsureError::AdmissionClosed) },
    )
    .await;
    assert!(matches!(result, Err(EnsureError::AdmissionClosed)));
    assert!(matches!(
        scope.begin_session_poll(intent_id, &allocation).await,
        Err(_)
    ));
    Ok(())
}

#[tokio::test]
async fn failed_queue_ack_after_inbox_write_holds_the_header_attempt() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 77,
        raw_body: r#"[{"messageType":"JobStarted","runnerRequestId":0}]"#.to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Started,
            request_id: None,
            runner_id: None,
            runner_name: None,
            result: None,
            job_id: None,
            labels: Vec::new(),
            fields: Vec::new(),
            source: SourceMetadata::default(),
        }],
    });
    let ack_attempted = Cell::new(false);
    let inbox_stored_at_ack = Cell::new(false);
    let handling = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        let pending = scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        inbox_stored_at_ack.set(pending.iter().any(|entry| entry.message_id == 77));
        ack_attempted.set(true);
        Err(EnsureError::AdmissionClosed)
    });

    assert!(matches!(
        finish_scoped_poll_response(
            &scope,
            intent_id,
            10_101,
            "session-scoped-poll",
            &allocation,
            attempt,
            handling,
        )
        .await,
        Err(EnsureError::AdmissionClosed)
    ));
    assert!(ack_attempted.get());
    assert!(inbox_stored_at_ack.get());
    assert!(
        scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .any(|entry| entry.message_id == 77)
    );
    assert!(
        scope
            .begin_zero_header_poll(intent_id, &allocation)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn available_message_stays_unacknowledged_at_zero_header() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 71,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":88}]"#.to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Available,
            request_id: Some(88),
            runner_id: None,
            runner_name: None,
            result: None,
            job_id: Some("opaque-job".to_owned()),
            labels: Vec::new(),
            fields: Vec::new(),
            source: Default::default(),
        }],
    });
    let acknowledged = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(!decision.completion_only);
    assert!(!decision.session_delete_candidate);
    assert!(!acknowledged.get());
    Ok(())
}

#[tokio::test]
async fn mixed_available_and_completion_is_stored_without_acknowledgement() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 72,
        raw_body: r#"[{"messageType":"JobAvailable","runnerRequestId":89},{"messageType":"JobCompleted","runnerRequestId":0,"runnerId":902,"runnerName":"v0"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Available,
                request_id: Some(89),
                runner_id: None,
                runner_name: None,
                result: None,
                job_id: Some("opaque-job".to_owned()),
                labels: Vec::new(),
                fields: Vec::new(),
                source: Default::default(),
            },
            InnerJob {
                kind: InnerKind::Completed,
                request_id: Some(0),
                runner_id: Some(902),
                runner_name: Some("v0".to_owned()),
                result: Some("Succeeded".to_owned()),
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: Default::default(),
            },
        ],
    });
    let acknowledged = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(!decision.completion_only);
    assert!(!decision.session_delete_candidate);
    assert!(!acknowledged.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 72);

    // A later replay cannot discard the Available message just because its
    // lifecycle sibling can be recorded.
    let replay_acknowledged = Cell::new(false);
    intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        replay_acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(!replay_acknowledged.get());
    let still_pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(still_pending.len(), 1);
    assert_eq!(still_pending[0].raw_body, pending[0].raw_body);
    Ok(())
}

#[tokio::test]
async fn started_only_event_is_stored_before_ack_and_replay_is_idempotent() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, session_intent, _) = session_scope(&scratch).await?;
    let polled = Poll::Batch(ParsedBatch {
        message_id: 73,
        raw_body:
            r#"[{"messageType":"JobStarted","runnerRequestId":0,"runnerId":902,"runnerName":"v0"}]"#
                .to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Started,
            request_id: None,
            runner_id: Some(902),
            runner_name: Some("v0".to_owned()),
            result: None,
            job_id: Some("opaque-job".to_owned()),
            labels: Vec::new(),
            fields: Vec::new(),
            source: SourceMetadata::default(),
        }],
    });
    let acknowledged = Cell::new(false);
    let inbox_stored_before_ack = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        let pending = scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        inbox_stored_before_ack.set(pending.iter().any(|entry| entry.message_id == 73));
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(decision.completion_only);
    assert!(decision.session_delete_candidate);
    assert!(acknowledged.get());
    assert!(inbox_stored_before_ack.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 73);
    assert!(
        !scope
            .session_work_drained(session_intent)
            .await
            .map_err(|error| error.to_string())?
    );

    let replay_acknowledged = Cell::new(false);
    intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        replay_acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(replay_acknowledged.get());
    let replayed = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].raw_body, pending[0].raw_body);
    Ok(())
}

#[tokio::test]
async fn partially_recorded_lifecycle_batch_is_acked_only_after_exact_inbox_write()
-> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, session_intent, _) = session_scope(&scratch).await?;
    let request_id = 88;
    let identity = record_acquired_request(&scope, request_id, 74).await?;

    let polled = Poll::Batch(ParsedBatch {
        message_id: 74,
        raw_body: r#"[{"messageType":"JobCompleted","runnerRequestId":88,"result":"cancelled"},{"messageType":"JobStarted","runnerRequestId":999,"runnerId":903,"runnerName":"v1"}]"#.to_owned(),
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Completed,
                request_id: Some(request_id),
                runner_id: None,
                runner_name: None,
                result: Some("cancelled".to_owned()),
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: SourceMetadata::default(),
            },
            InnerJob {
                kind: InnerKind::Started,
                request_id: Some(999),
                runner_id: Some(903),
                runner_name: Some("v1".to_owned()),
                result: None,
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: SourceMetadata::default(),
            },
        ],
    });
    let acknowledged = Cell::new(false);
    let inbox_stored_before_ack = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        let pending = scope
            .pending_completion_inbox(4_102_444_800, 10)
            .await
            .map_err(|_| EnsureError::AdmissionClosed)?;
        inbox_stored_before_ack.set(pending.iter().any(|entry| entry.message_id == 74));
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(decision.completion_only);
    assert!(acknowledged.get());
    assert!(inbox_stored_before_ack.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 74);
    assert!(
        !scope
            .session_work_drained(session_intent)
            .await
            .map_err(|error| error.to_string())?
    );
    assert!(
        scope
            .record_job_completed(
                &identity,
                74,
                &SourceMetadata::default(),
                Some("succeeded"),
                None,
                None,
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn empty_poll_is_delete_candidate_but_has_no_event_ack() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        attempt,
        async {
            Ok(IntakeDecision {
                completion_only: false,
                wake_cleanup: false,
                session_delete_candidate: true,
            })
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(!outcome.wake_cleanup);
    assert!(outcome.session_delete_candidate);
    assert!(
        scope
            .session_work_drained(intent_id)
            .await
            .map_err(|error| error.to_string())?
    );
    Ok(())
}

#[tokio::test]
async fn quarantined_body_is_saved_but_not_acknowledged_or_deleted() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, session_intent, _) = session_scope(&scratch).await?;
    let polled = Poll::Quarantined(velnor_runner_github::QuarantinedBatch {
        message_id: 76,
        raw_body: r#"{"malformed":"inner message body"}"#.to_owned(),
    });
    let acknowledged = Cell::new(false);

    let decision = intake_and_ack_if_only_scoped(&scope, 10_101, &polled, || async {
        acknowledged.set(true);
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    assert!(!decision.completion_only);
    assert!(!decision.session_delete_candidate);
    assert!(!acknowledged.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 76);
    assert_eq!(pending[0].raw_body, r#"{"malformed":"inner message body"}"#);
    assert!(
        !scope
            .session_work_drained(session_intent)
            .await
            .map_err(|error| error.to_string())?
    );
    Ok(())
}

#[tokio::test]
async fn reused_message_id_with_changed_body_is_not_acknowledged() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _, _) = session_scope(&scratch).await?;
    let original = Poll::Batch(ParsedBatch {
        message_id: 75,
        raw_body:
            r#"[{"messageType":"JobStarted","runnerRequestId":0,"runnerId":904,"runnerName":"v2"}]"#
                .to_owned(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Started,
            request_id: None,
            runner_id: Some(904),
            runner_name: Some("v2".to_owned()),
            result: None,
            job_id: None,
            labels: Vec::new(),
            fields: Vec::new(),
            source: SourceMetadata::default(),
        }],
    });
    intake_and_ack_if_only_scoped(&scope, 10_101, &original, || async {
        Ok::<(), EnsureError>(())
    })
    .await
    .map_err(|error| error.to_string())?;

    let mut changed = original.clone();
    let Poll::Batch(batch) = &mut changed else {
        return Err("expected batch".to_owned());
    };
    batch.raw_body =
        r#"[{"messageType":"JobStarted","runnerRequestId":0,"runnerId":905,"runnerName":"v3"}]"#
            .to_owned();
    batch.jobs[0].runner_id = Some(905);
    batch.jobs[0].runner_name = Some("v3".to_owned());
    let acknowledged = Cell::new(false);
    assert!(
        intake_and_ack_if_only_scoped(&scope, 10_101, &changed, || async {
            acknowledged.set(true);
            Ok::<(), EnsureError>(())
        })
        .await
        .is_err()
    );
    assert!(!acknowledged.get());
    Ok(())
}
