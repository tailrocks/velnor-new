//! Launch ordering. The scripted transport is the acquire, JIT, and ack path.

use std::sync::{Arc, Mutex};

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics};

use crate::journal::Outcome;
use crate::launch::{Idle, drive_offer, fail_unstarted, idle};
use crate::launch_harness::{
    CANARY, Mode, Script, absent, assigned_wait, available, ctx, open, started_progress,
    started_wait,
};
use crate::launch_test_support::valid_worker_volume;
use crate::{EnsureError, HostError, IntentState, Started};

#[path = "launch_drive_tests.rs"]
mod drive;

#[test]
fn statistics_advance_and_offers_stay() {
    let stats = Poll::Batch(ParsedBatch {
        message_id: 2,
        raw_body: String::new(),
        statistics: None,
        jobs: Vec::new(),
    });
    assert_eq!(idle(&stats), Idle::Ack);
    assert_eq!(idle(&available(&[3])), Idle::Launch);
    assert_eq!(idle(&Poll::Empty), Idle::Empty);
    assert_eq!(idle(&available(&[3, 4])), Idle::Blocked);
    assert_eq!(idle(&assigned_wait(7, 1)), Idle::Scale);
    assert_eq!(idle(&assigned_wait(7, 0)), Idle::Ack);
    assert_eq!(idle(&started_wait(7, 1)), Idle::Scale);
    assert_eq!(idle(&assigned_started(7, 4)), Idle::Ack);
    assert_eq!(idle(&assigned_wait(8, -1)), Idle::Blocked);
    assert_eq!(idle(&started_progress(11, 5)), Idle::Scale);

    assert_eq!(idle(&no_stats(vec![job(InnerKind::Started)])), Idle::Ack);
    assert_eq!(idle(&no_stats(vec![job(InnerKind::Completed)])), Idle::Ack);
    assert_eq!(
        idle(&no_stats(vec![
            job(InnerKind::Started),
            job(InnerKind::Completed),
        ])),
        Idle::Ack
    );
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Assigned)])),
        Idle::Blocked
    );
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Available)])),
        Idle::Blocked
    );
    assert_eq!(
        idle(&no_stats(vec![
            job(InnerKind::Started),
            job(InnerKind::Available),
        ])),
        Idle::Blocked
    );
    let mut available_with_id = job(InnerKind::Available);
    available_with_id.request_id = Some(9);
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Started), available_with_id])),
        Idle::Launch
    );
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Unsupported(
            "FutureKind".to_owned(),
        ))])),
        Idle::Blocked
    );
    assert_eq!(
        idle(&batch(
            Some(Statistics {
                total_available_jobs: 0,
                total_acquired_jobs: 0,
                total_assigned_jobs: -1,
                total_running_jobs: 0,
                total_registered_runners: 0,
                total_busy_runners: 0,
                total_idle_runners: 0,
            }),
            vec![job(InnerKind::Started)],
        )),
        Idle::Blocked
    );
    let synthetic = ParsedBatch {
        message_id: -1,
        raw_body: String::new(),
        statistics: None,
        jobs: Vec::new(),
    };
    assert_eq!(idle(&Poll::Batch(synthetic)), Idle::Blocked);
}

fn no_stats(jobs: Vec<InnerJob>) -> Poll {
    batch(None, jobs)
}

fn batch(statistics: Option<Statistics>, jobs: Vec<InnerJob>) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 77,
        raw_body: String::new(),
        statistics,
        jobs,
    })
}

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

#[tokio::test]
async fn launch_acks_only_after_start_and_hides_jit() -> Result<(), String> {
    let (scratch, journal) = open("ok").await?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let started = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |volume, jit, _bind| {
            let volume = volume.to_owned();
            let jit = jit.to_vec();
            let captured = Arc::clone(&captured);
            async move {
                if !valid_worker_volume(&volume) {
                    return Err(HostError::ForbiddenMount);
                }
                let mut slot = captured.lock().map_err(|_| HostError::Docker)?;
                *slot = jit;
                Ok(Started {
                    dind_id: "dind-1".to_owned(),
                    runner_id: "runner-1".to_owned(),
                })
            }
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    let started = started.ok_or_else(|| "missing worker".to_owned())?;
    assert_eq!(started.runner_id, "runner-1");
    assert_eq!(script.calls, ["acquire", "jit", "ack"]);
    {
        let slot = seen.lock().map_err(|err| err.to_string())?;
        assert_eq!(slot.as_slice(), CANARY.as_bytes());
    }
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-1"));
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-1"));
    assert_eq!(
        journal
            .record_runner_completed(1, 3, 901, "v3")
            .await
            .map_err(|error| error.to_string())?,
        Some(rows[0].id)
    );
    absent(&scratch.file())
}

#[tokio::test]
async fn uncertain_acquire_does_not_ack() -> Result<(), String> {
    let (scratch, journal) = open("timeout").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Timeout,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await
    .map_err(|err| err.to_string());
    assert_eq!(error, Err("effect uncertain".to_owned()));
    assert_eq!(script.calls, ["acquire"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id, None);
    let id = rows[0].id;
    let mut replay = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let replayed = drive_offer(
        &mut replay,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(replayed, Err(EnsureError::Uncertain));
    assert_eq!(replay.calls, Vec::<&str>::new());
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn forbidden_acquire_is_failed_and_not_acked() -> Result<(), String> {
    let (_scratch, journal) = open("forbidden").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Forbidden,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(error, Err(EnsureError::Forbidden));
    assert_eq!(script.calls, ["acquire"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Failed);
    Ok(())
}

#[tokio::test]
async fn empty_acquire_is_not_acked() -> Result<(), String> {
    let (_scratch, journal) = open("empty").await?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Empty,
    };
    let error = drive_offer(
        &mut script,
        &ctx(),
        &available(&[3]),
        &journal,
        |_volume, _jit, _bind| async { Err(HostError::Docker) },
    )
    .await;
    assert_eq!(
        error,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "acquire",
        })
    );
    assert_eq!(script.calls, ["acquire"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert_eq!(rows[0].state, IntentState::Failed);
    Ok(())
}
