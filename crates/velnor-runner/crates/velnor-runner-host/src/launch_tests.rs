//! Legacy-row cleanup guards.

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics};

use crate::IntentState;
use crate::journal::Outcome;
use crate::launch::fail_unstarted;
use crate::launch_harness::{absent, assigned_wait, open};

fn job(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: None,
        labels: Vec::new(),
        runner_id: None,
        runner_name: None,
        result: None,
        fields: Vec::new(),
        source: velnor_runner_github::SourceMetadata::default(),
    }
}

fn assigned_started(message_id: i64, assigned: i64) -> Poll {
    let mut assigned_job = job(InnerKind::Assigned);
    assigned_job.request_id = Some(4);
    let mut started_job = job(InnerKind::Started);
    started_job.request_id = Some(4);
    Poll::Batch(ParsedBatch {
        message_id,
        raw_body: String::new(),
        statistics: Some(Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs: assigned,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        }),
        jobs: vec![assigned_job, started_job],
    })
}

#[tokio::test]
async fn started_replay_releases_the_unstarted_row() -> Result<(), String> {
    let (scratch, journal) = open("started-replay").await?;
    let id = journal
        .begin("launch", "m100000776")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let poll = assigned_started(100_000_776, 4);
    let failed = fail_unstarted(&journal, &poll)
        .await
        .map_err(|err| err.to_string())?;
    if !failed {
        return Err("unstarted replay row was not failed".to_owned());
    }
    let state = journal.read(id).await.map_err(|err| err.to_string())?;
    if state != IntentState::Failed {
        return Err(format!("state {state:?}"));
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn name_taken_failure_releases_the_unstarted_scale_row() -> Result<(), String> {
    let (scratch, journal) = open("name-taken").await?;
    let id = journal
        .begin("launch", "m100000769")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    let failed = fail_unstarted(&journal, &assigned_wait(100_000_769, 5))
        .await
        .map_err(|err| err.to_string())?;
    if !failed {
        return Err("unstarted row was not failed".to_owned());
    }
    let state = journal.read(id).await.map_err(|err| err.to_string())?;
    if state != IntentState::Failed {
        return Err(format!("state {state:?}"));
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn name_taken_keeps_a_row_that_has_a_container() -> Result<(), String> {
    let (scratch, journal) = open("name-taken-live").await?;
    let id = journal
        .begin("launch", "m100000769")
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .bind_worker(id, Some("runner-container"), None)
        .await
        .map_err(|err| err.to_string())?;
    let failed = fail_unstarted(&journal, &assigned_wait(100_000_769, 5))
        .await
        .map_err(|err| err.to_string())?;
    if failed {
        return Err("container row was treated as a name collision".to_owned());
    }
    let state = journal.read(id).await.map_err(|err| err.to_string())?;
    if state != IntentState::Uncertain {
        return Err(format!("state {state:?}"));
    }
    absent(&scratch.file())
}
