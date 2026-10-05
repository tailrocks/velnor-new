//! Regressions for the same start path used by `Turn::start`.

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail, create_session,
};

use super::{Ready, start_turn};
use crate::journal::Outcome;
use crate::launch::inspect_tests::{DockerStub, http};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};
use crate::worker::Started;
use crate::{EnsureError, IntentState};

const INITIAL_SESSION: &[u8] = br#"{"sessionId":"session","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":0,"totalAssignedJobs":0,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;

struct InitialSession;

impl Transport for InitialSession {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        Ok(Exchange {
            status: 200,
            body: INITIAL_SESSION.to_vec(),
        })
    }
}

fn zero_assignment_session() -> Result<QueueSession, String> {
    create_session(&mut InitialSession, 1, "owner", "admin-token")
        .map_err(|error| error.to_string())
}

fn ready<'a>(session: &'a QueueSession, polled: &'a Poll) -> Ready<'a> {
    Ready {
        set_id: 1,
        session,
        admin_token: "admin-token",
        path: "messages".to_owned(),
        polled,
    }
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
async fn unlabeled_volume_keeps_cleanup_unproven_and_redelivery_unacked() -> Result<(), String> {
    let (scratch, journal) = open("turn-unlabeled-volume").await?;
    let (row, _) = journal
        .begin_launch("m95")
        .await
        .map_err(|error| error.to_string())?;
    let worker = "w00000000000000000000000000000000";
    journal
        .bind_worker_volume(row, worker)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let session = zero_assignment_session()?;
    let polled = assigned_wait(95, 1);
    let unlabeled = serde_json::json!({
        "Name": worker,
        "Driver": "local",
        "Mountpoint": "/var/lib/docker/volumes/w/_data",
        "Labels": {},
        "Options": {},
        "Scope": "local"
    })
    .to_string();
    let docker = DockerStub::open(vec![
        http(404, r#"{"message":"missing"}"#),
        http(404, r#"{"message":"missing"}"#),
        http(200, &unlabeled),
    ])?;
    let decision = crate::launch::admission(&docker.docker, &journal, 1, 1, 0, &polled).await;
    let requests = docker.finish().await?;

    assert_eq!(decision, Ok(crate::launch::Admit::Hold));
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].worker_volume.as_deref(), Some(worker));
    assert!(!rows[0].cleanup_proven);

    let docker = DockerStub::open(Vec::new())?;
    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::JitConflict,
    };
    let mut workers: Vec<Started> = Vec::new();
    let redelivered = start_turn(
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

    assert_eq!(redelivered, Ok(false));
    assert!(script.calls.is_empty());
    assert!(workers.is_empty());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].worker_volume.as_deref(), Some(worker));
    assert!(!rows[0].cleanup_proven);
    absent(&scratch.file())
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
        &crate::launch_test_support::Engine::new(),
        &journal,
        2,
        2,
        0,
        &polled,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(decision, crate::launch::Admit::Error);
    assert!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
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
    let docker = DockerStub::open(vec![http(
        200,
        r#"{"State":{"Status":"running","Running":true}}"#,
    )])?;
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

    assert_eq!(result, Ok(false));
    assert_eq!(script.calls, ["ack"]);
    assert!(workers.is_empty());
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

    assert_eq!(result, Err(EnsureError::Uncertain));
    assert!(script.calls.is_empty());
    assert!(workers.is_empty());
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

#[path = "start_admission_tests.rs"]
mod admission_tests;
