//! Completion messages become durable before the poll path can acknowledge them.

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use super::completion_intake::{
    completion_only, intake_and_ack_if_only, intake_poll, record_completed, retry_quarantined,
};
use crate::journal::MAX_COMPLETION_BODY_BYTES;
use crate::launch_harness::open;

fn completed(request_id: Option<i64>, runner_id: Option<i64>, name: Option<&str>) -> Poll {
    let raw_body = completion_body(request_id, runner_id, name);
    Poll::Batch(ParsedBatch {
        message_id: 50,
        raw_body,
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Completed,
            request_id,
            job_id: None,
            labels: Vec::new(),
            runner_id,
            runner_name: name.map(str::to_owned),
            result: Some("Succeeded".to_owned()),
            fields: Vec::new(),
        }],
    })
}

fn completion_body(request_id: Option<i64>, runner_id: Option<i64>, name: Option<&str>) -> String {
    let mut fields = vec!["\"messageType\":\"JobCompleted\"".to_owned()];
    if let Some(request_id) = request_id {
        fields.push(format!("\"runnerRequestId\":{request_id}"));
    }
    if let Some(runner_id) = runner_id {
        fields.push(format!("\"runnerId\":{runner_id}"));
    }
    if let Some(name) = name {
        fields.push(format!("\"runnerName\":\"{name}\""));
    }
    format!("[{{{}}}]", fields.join(","))
}

#[tokio::test]
async fn duplicate_completion_is_persisted_once_before_ack_path() -> Result<(), String> {
    let (_scratch, journal) = open("completion-intake").await?;
    let (id, fresh) = journal
        .begin_assigned_launch("m49r61", 7, 61, "v61")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    let event = completed(Some(61), Some(901), Some("v61"));

    let first_intake = intake_poll(&journal, 7, &event)
        .await
        .map_err(|error| error.to_string())?;
    let replay_intake = intake_poll(&journal, 7, &event)
        .await
        .map_err(|error| error.to_string())?;
    assert!(first_intake.completion_only && first_intake.wake_cleanup);
    assert!(replay_intake.completion_only && replay_intake.wake_cleanup);

    let due = journal
        .due_completed_launches(0, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].intent.id, id);
    assert_eq!(due[0].identity.runner_id, 901);
    Ok(())
}

#[tokio::test]
async fn unmatched_completion_is_quarantined_without_claiming_cleanup_authority()
-> Result<(), String> {
    let (_scratch, journal) = open("completion-unowned").await?;
    let (id, fresh) = journal
        .begin_assigned_launch("m51r61", 7, 61, "v61")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);

    record_completed(&journal, 7, &completed(Some(61), Some(901), None))
        .await
        .map_err(|error| error.to_string())?;
    record_completed(&journal, 8, &completed(Some(61), Some(901), Some("v61")))
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .due_completed_launches(0, 10)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    assert!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .any(|row| row.id == id && !row.cleanup_proven)
    );
    let inbox = journal
        .pending_completion_inbox(0, 4)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(inbox.len(), 2);
    assert_eq!(
        inbox[0].raw_body,
        completion_body(Some(61), Some(901), None)
    );
    Ok(())
}

#[tokio::test]
async fn invalid_completion_identity_is_quarantined_without_journal_authority() -> Result<(), String>
{
    let (_scratch, journal) = open("completion-inbox-invalid-identity").await?;
    let event = completed(Some(0), Some(901), Some("v0"));
    record_completed(&journal, 7, &event)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .due_completed_launches(0, 10)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    let inbox = journal
        .pending_completion_inbox(0, 4)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0].raw_body,
        completion_body(Some(0), Some(901), Some("v0"))
    );
    Ok(())
}

#[tokio::test]
async fn negative_completion_id_fails_before_authority_or_ack() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-invalid-intake").await?;
    let (launch_id, fresh) = journal
        .begin_assigned_launch("m53r61", 7, 61, "v61")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    let Poll::Batch(mut batch) = completed(Some(61), Some(901), Some("v61")) else {
        return Err("batch".to_owned());
    };
    batch.message_id = -1;
    let invalid_id = Poll::Batch(batch);
    assert!(!completion_only(&invalid_id));
    assert!(record_completed(&journal, 7, &invalid_id).await.is_err());
    let mut acknowledged = false;
    assert!(
        intake_and_ack_if_only(&journal, 7, &invalid_id, || async {
            acknowledged = true;
            Ok(())
        })
        .await
        .is_err()
    );
    assert!(!acknowledged);
    assert!(
        journal
            .due_completed_launches(0, 10)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    assert!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .any(|row| row.id == launch_id && !row.cleanup_proven)
    );
    Ok(())
}

#[tokio::test]
async fn oversized_completion_data_fails_before_inbox_write() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-oversized-intake").await?;
    let oversized = Poll::Batch(ParsedBatch {
        message_id: 51,
        raw_body: "x".repeat(MAX_COMPLETION_BODY_BYTES + 1),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Completed,
            request_id: None,
            job_id: None,
            labels: Vec::new(),
            runner_id: None,
            runner_name: None,
            result: None,
            fields: Vec::new(),
        }],
    });
    assert!(record_completed(&journal, 7, &oversized).await.is_err());
    assert!(
        journal
            .pending_completion_inbox(0, 4)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );

    let Poll::Batch(mut too_many) = completed(Some(61), Some(901), Some("v61")) else {
        return Err("batch".to_owned());
    };
    let message = too_many.jobs.first().ok_or("completion")?.clone();
    too_many.jobs = vec![message; velnor_runner_github::MAX_POLL_MESSAGES + 1];
    assert!(
        record_completed(&journal, 7, &Poll::Batch(too_many))
            .await
            .is_err()
    );
    assert!(
        journal
            .pending_completion_inbox(0, 4)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn quarantined_completion_retries_after_legacy_identity_appears() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-retry").await?;
    let event = completed(Some(61), Some(901), Some("v61"));
    record_completed(&journal, 7, &event)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .due_completed_launches(0, 10)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );

    let (id, fresh) = journal
        .begin_assigned_launch("m52r61", 7, 61, "v61")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    assert!(
        retry_quarantined(&journal)
            .await
            .map_err(|error| error.to_string())?
    );
    let due = journal
        .due_completed_launches(i64::MAX, 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].intent.id, id);
    assert_eq!(due[0].identity.runner_id, 901);
    assert!(
        journal
            .pending_completion_inbox(i64::MAX, 4)
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}
