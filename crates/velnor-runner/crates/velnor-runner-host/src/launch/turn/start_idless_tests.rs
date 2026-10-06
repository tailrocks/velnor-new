//! An idless uncertain row stays occupied on the turn path.

use velnor_runner_github::Poll;

use super::start_tests::{ready, rest, zero_assignment_session};
use super::{StartTurn, start_turn};
use crate::IntentState;
use crate::journal::Outcome;
use crate::launch::inspect_tests::DockerStub;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, ctx, open, started_progress};
use crate::worker::Started;

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
        StartTurn {
            ready: ready(&session, &polled),
            journal: &journal,
            docker: &docker.docker,
            capacity: 1,
            rest: rest(),
            stop: false,
        },
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
    super::super::steps::acknowledge(&mut script, &ctx(), batch)
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
