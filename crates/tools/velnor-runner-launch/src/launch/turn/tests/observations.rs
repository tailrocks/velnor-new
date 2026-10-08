//! Queue lifecycle events remain durable even when an offer is held.

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};
use velnor_runner_journal::journal::Outcome;

use crate::launch::harness::open;

#[tokio::test]
async fn started_and_completed_identities_are_persisted_before_admission() -> Result<(), String> {
    let (_scratch, journal) = open("poll-observations").await?;
    let (launch_id, fresh) = journal
        .begin_launch("held-offer")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_launch_identity(launch_id, None, None, None, None, "runner-actual")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(launch_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(launch_id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;

    let poll = Poll::Batch(ParsedBatch {
        message_id: 103,
        statistics: None,
        jobs: vec![
            lifecycle_event(InnerKind::Started),
            lifecycle_event(InnerKind::Completed),
        ],
    });
    super::super::observations::persist_observed_lifecycle(&journal, &poll)
        .await
        .map_err(|error| error.to_string())?;

    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "observed launch row disappeared".to_owned())?;
    assert_eq!(row.github_runner_id.as_deref(), Some("81"));
    assert_eq!(row.observed_job_id.as_deref(), Some("scale-job-opaque"));
    assert_eq!(row.observed_workflow_run_id, Some(9001));
    assert!(row.remote_terminal);
    Ok(())
}

fn lifecycle_event(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: Some("scale-job-opaque".to_owned()),
        workflow_run_id: Some(9001),
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: Some(81),
        runner_name: Some("runner-actual".to_owned()),
        result: None,
        fields: Vec::new(),
    }
}
