//! Census, status, and uncertain-slot admission regressions for `Turn::start`.

use velnor_runner_github::Poll;

use super::{ready, start_turn, zero_assignment_session};
use crate::journal::Outcome;
use crate::launch::inspect_tests::{DockerStub, http};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, ctx, open, started_progress};
use crate::worker::Started;
use crate::{EnsureError, IntentState};

#[tokio::test]
async fn zero_initial_census_and_positive_poll_keep_jit_conflict_unacked() -> Result<(), String> {
    let (scratch, journal) = open("turn-census-conflict").await?;
    let session = zero_assignment_session()?;
    assert_eq!(
        session
            .statistics()
            .map(velnor_runner_github::Statistics::assigned_population),
        Some(0)
    );
    let polled = assigned_wait(91, 1);
    assert_eq!(crate::launch::idle(&polled), crate::launch::Idle::Scale);

    let docker = DockerStub::open(Vec::new())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers: Vec<Started> = Vec::new();
    let result = start_turn(
        &mut script,
        &mut workers,
        ready(&session, &polled),
        &journal,
        &docker.docker,
        2,
        false,
    )
    .await;
    drop(docker);

    assert_eq!(result, Err(EnsureError::Conflict));
    assert_eq!(script.calls, ["jit"]);
    assert!(workers.is_empty());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].subject, "m91");
    assert_eq!(rows[0].state, IntentState::Failed);
    assert!(rows[0].docker_id.is_none());
    assert!(rows[0].dind_id.is_none());
    assert!(rows[0].worker_volume.is_none());
    absent(&scratch.file())
}

#[tokio::test]
async fn missing_status_keeps_the_slot_and_blocks_assignment() -> Result<(), String> {
    let (scratch, journal) = open("turn-missing-runner-status").await?;
    let (row, _) = journal
        .begin_launch("m96")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(row, Some("runner-container"), None)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(96, 1);
    let docker = DockerStub::open(vec![http(200, r#"{"State":{"Running":false}}"#)])?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    let mut workers: Vec<Started> = Vec::new();
    let result = start_turn(
        &mut script,
        &mut workers,
        ready(&session, &polled),
        &journal,
        &docker.docker,
        2,
        false,
    )
    .await;
    let requests = docker.finish().await?;

    assert_eq!(
        result,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "docker",
        })
    );
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("/containers/runner-container/json"));
    assert!(script.calls.is_empty());
    assert!(workers.is_empty());
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(1));
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].state, IntentState::Uncertain);
    assert_eq!(after[0].docker_id.as_deref(), Some("runner-container"));
    absent(&scratch.file())
}

#[tokio::test]
async fn idless_uncertain_reservation_blocks_turn_without_jit_or_ack() -> Result<(), String> {
    let (scratch, journal) = open("turn-idless-uncertain").await?;
    let (row, _) = journal
        .begin_launch("m95")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(95, 1);
    let docker = DockerStub::open(Vec::new())?;
    let decision = crate::launch::admission(&docker.docker, &journal, 1, 1, 0, &polled)
        .await
        .map_err(|error| error.to_string())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers: Vec<Started> = Vec::new();
    let result = start_turn(
        &mut script,
        &mut workers,
        ready(&session, &polled),
        &journal,
        &docker.docker,
        1,
        false,
    )
    .await;
    docker.finish().await?;

    assert_eq!(decision, crate::launch::Admit::Hold);
    assert_eq!(result, Ok(false));
    assert!(script.calls.is_empty());
    assert!(workers.is_empty());
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(1));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].subject, "m95");
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert!(rows[0].docker_id.is_none());
    assert!(rows[0].dind_id.is_none());
    assert!(rows[0].worker_volume.is_none());
    absent(&scratch.file())
}

#[tokio::test]
async fn full_uncertain_slot_still_acks_progress_notice() -> Result<(), String> {
    let (scratch, journal) = open("turn-progress-uncertain").await?;
    let (row, _) = journal
        .begin_launch("m95")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let polled = started_progress(95, 5);
    let decision = crate::launch::admission(
        &crate::launch_test_support::Engine::new(),
        &journal,
        1,
        1,
        0,
        &polled,
    )
    .await
    .map_err(|error| error.to_string())?;
    let Poll::Batch(batch) = &polled else {
        return Err("progress fixture must contain a batch".to_owned());
    };
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    super::super::super::steps::acknowledge(&mut script, &ctx(), batch)
        .map_err(|error| error.to_string())?;

    assert_eq!(decision, crate::launch::Admit::Ack { stop: false });
    assert_eq!(script.calls, ["ack"]);
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(1));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].subject, "m95");
    assert_eq!(rows[0].state, IntentState::Uncertain);
    absent(&scratch.file())
}
