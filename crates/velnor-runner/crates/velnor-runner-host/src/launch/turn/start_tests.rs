//! Regressions for the same start path used by `Turn::start`.

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail, create_session,
};

use super::{Ready, StartTurn, start_turn};
use crate::journal::Outcome;
use crate::launch::inspect_tests::{DockerStub, http};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};
use crate::worker::Started;
use crate::{EnsureError, IntentState};

mod guest_admission;
mod legacy_failed;
mod zero_census_conflict;

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

pub(super) fn zero_assignment_session() -> Result<QueueSession, String> {
    create_session(&mut InitialSession, 1, "owner", "admin-token")
        .map_err(|error| error.to_string())
}

pub(super) fn rest() -> crate::launch::Rest<'static> {
    crate::launch::Rest {
        owner: "",
        repo: "",
        pat: "",
        resource_budget: crate::worker::test_resource_budget().ok(),
        static_capacity: true,
        guest_admission: crate::launch::drive::GuestAdmission::FreshSample,
    }
}

pub(super) fn ready<'a>(session: &'a QueueSession, polled: &'a Poll) -> Ready<'a> {
    Ready {
        set_id: 1,
        session,
        admin_token: "admin-token",
        path: "messages".to_owned(),
        polled,
    }
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
        StartTurn {
            ready: ready(&session, &polled),
            journal: &journal,
            docker: &docker.docker,
            capacity: 2,
            rest: rest(),
            stop: false,
        },
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
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [worker::projection_types::Started; 0]);
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(1));
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].state, IntentState::Uncertain);
    assert_eq!(after[0].docker_id.as_deref(), Some("runner-container"));
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
async fn uncertain_volume_keeps_cleanup_unproven_and_redelivery_unacked() -> Result<(), String> {
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
    let docker = DockerStub::open(Vec::new())?;
    let decision = crate::launch::admission(&docker.docker, &journal, 1, 1, 0, &polled).await;
    let requests = docker.finish().await?;

    assert_eq!(decision, Ok(crate::launch::Admit::Hold));
    assert_eq!(requests, [] as [std::string::String; 0]);
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

    assert_eq!(redelivered, Ok(false));
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [worker::projection_types::Started; 0]);
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
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        [] as [reconcile::IntentRow; 0]
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
    let docker = DockerStub::open(vec![
        http(200, r#"{"State":{"Status":"running","Running":true}}"#),
        // Bind the fixture sample to the same selected daemon and root.
        http(
            200,
            r#"{"ID":"test-engine","DockerRootDir":"/var/lib/docker"}"#,
        ),
    ])?;
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
            capacity: 2,
            rest: rest(),
            stop: false,
        },
    )
    .await;
    docker.finish().await?;

    assert_eq!(result, Ok(false));
    assert_eq!(script.calls, ["ack"]);
    assert_eq!(workers, [] as [worker::projection_types::Started; 0]);
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
    let docker = DockerStub::open(vec![http(
        200,
        r#"{"ID":"test-engine","DockerRootDir":"/var/lib/docker"}"#,
    )])?;
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
            capacity: 2,
            rest: rest(),
            stop: false,
        },
    )
    .await;
    drop(docker);

    assert_eq!(result, Err(EnsureError::Uncertain));
    assert_eq!(script.calls, [] as [&str; 0]);
    assert_eq!(workers, [] as [worker::projection_types::Started; 0]);
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
