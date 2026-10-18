use std::cell::Cell;
use std::path::PathBuf;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::journal::{CapacityLimits, ConnectionJournal, HeaderAllocation, Journal};
use crate::scale_set::EnsureError;

use super::completion_intake::intake_and_ack_if_only_scoped;
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
    scope
        .bind_scale_set(10_101, "integration", SetOwnership::Adopted)
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

#[tokio::test]
async fn handled_zero_header_response_settles_exact_poll_attempt() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let attempt = scope
        .begin_session_poll(intent_id, &allocation)
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
        async { Ok(true) },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(wake);

    let replay = scope
        .begin_session_poll(intent_id, &allocation)
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
        .begin_session_poll(intent_id, &allocation)
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
    assert!(!acknowledged.get());
    let pending = scope
        .pending_completion_inbox(4_102_444_800, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message_id, 72);
    Ok(())
}
