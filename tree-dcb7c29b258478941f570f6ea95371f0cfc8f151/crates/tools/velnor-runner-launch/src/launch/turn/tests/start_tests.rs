//! Regressions for the same start path used by `Turn::start`.

use velnor_runner_github::Poll;

use super::super::start_turn;
use super::{ready, zero_assignment_session};
use crate::launch::docker_stub::{DockerStub, http};
use crate::launch::harness::{Mode, Script, absent, assigned_wait, ctx, open, started_progress};
use velnor_runner_host::journal::Outcome;
use velnor_runner_host::worker::Started;
use velnor_runner_host::{EnsureError, IntentState};

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

    assert!(matches!(result, Err(EnsureError::Conflict)));
    assert_eq!(script.calls, ["jit"]);
    assert_eq!(workers, Vec::<Started>::new());
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
    assert_eq!(result.map(|outcome| outcome.stop), Ok(false));
    assert_eq!(script.calls, Vec::<&'static str>::new());
    assert_eq!(workers, Vec::<Started>::new());
    assert_eq!(velnor_runner_launch_slot::occupied(&journal).await, Ok(1));
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
        &crate::launch::fakes::Engine::new(),
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
    crate::launch::steps::acknowledge(&mut script, &ctx(), batch)
        .map_err(|error| error.to_string())?;

    assert_eq!(decision, crate::launch::Admit::Ack { stop: false });
    assert_eq!(script.calls, ["ack"]);
    assert_eq!(velnor_runner_launch_slot::occupied(&journal).await, Ok(1));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].subject, "m95");
    assert_eq!(rows[0].state, IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn dind_only_live_row_keeps_the_current_assignment_unacked() -> Result<(), String> {
    bound_resource_keeps_assignment("turn-dind-only", BoundResource::Dind).await
}

#[tokio::test]
async fn volume_only_live_row_keeps_the_current_assignment_unacked() -> Result<(), String> {
    bound_resource_keeps_assignment("turn-volume-only", BoundResource::Volume).await
}

#[tokio::test]
async fn missing_current_census_blocks_ack_and_start() -> Result<(), String> {
    let (scratch, journal) = open("turn-missing-census").await?;
    let mut polled = assigned_wait(94, 1);
    if let Poll::Batch(batch) = &mut polled {
        batch.statistics = None;
    }
    assert_eq!(crate::launch::idle(&polled), crate::launch::Idle::Blocked);
    let decision = crate::launch::admission(
        &crate::launch::fakes::Engine::new(),
        &journal,
        2,
        2,
        0,
        &polled,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(decision, crate::launch::Admit::Error);
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        Vec::<velnor_runner_host::reconcile::IntentRow>::new()
    );
    absent(&scratch.file())
}

#[tokio::test]
async fn bound_running_worker_acks_without_a_second_jit_request() -> Result<(), String> {
    let (scratch, journal) = open("turn-bound-runner").await?;
    let (row, _) = journal
        .begin_launch("m93")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(row, "w00000000000000000000000000000000")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(row, Some("runner-container"), Some("dind-container"))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(93, 1);
    let docker = DockerStub::open(vec![http(200, r#"{"State":{"Running":true}}"#)])?;
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
    docker.finish().await?;

    assert_eq!(result.map(|outcome| outcome.stop), Ok(false));
    assert_eq!(script.calls, ["ack"]);
    assert_eq!(workers, Vec::<Started>::new());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows[0].state, IntentState::Done);
    assert_eq!(rows[0].docker_id.as_deref(), Some("runner-container"));
    assert_eq!(rows[0].dind_id.as_deref(), Some("dind-container"));
    assert_eq!(
        rows[0].worker_volume.as_deref(),
        Some("w00000000000000000000000000000000")
    );
    absent(&scratch.file())
}

#[derive(Clone, Copy)]
enum BoundResource {
    Dind,
    Volume,
}

async fn bound_resource_keeps_assignment(
    label: &str,
    resource: BoundResource,
) -> Result<(), String> {
    let (scratch, journal) = open(label).await?;
    let (row, _) = journal
        .begin_launch("m92")
        .await
        .map_err(|error| error.to_string())?;
    let message_id = 92;
    match resource {
        BoundResource::Dind => journal
            .bind_worker(row, None, Some("dind-container"))
            .await
            .map_err(|error| error.to_string())?,
        BoundResource::Volume => journal
            .bind_worker_volume(row, "w00000000000000000000000000000000")
            .await
            .map_err(|error| error.to_string())?,
    }
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(message_id, 1);
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

    assert!(matches!(result, Err(EnsureError::Uncertain)));
    assert_eq!(script.calls, Vec::<&'static str>::new());
    assert_eq!(workers, Vec::<Started>::new());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    match resource {
        BoundResource::Dind => {
            assert_eq!(rows[0].dind_id.as_deref(), Some("dind-container"));
            assert!(rows[0].worker_volume.is_none());
        }
        BoundResource::Volume => {
            assert_eq!(
                rows[0].worker_volume.as_deref(),
                Some("w00000000000000000000000000000000")
            );
            assert!(rows[0].dind_id.is_none());
        }
    }
    absent(&scratch.file())
}
