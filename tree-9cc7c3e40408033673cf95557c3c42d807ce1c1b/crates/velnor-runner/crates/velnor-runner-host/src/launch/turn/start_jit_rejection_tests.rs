//! Definite JIT rejection can release only an unacquired population reservation.

use super::start_tests::{ready, rest, zero_assignment_session};
use super::{StartTurn, start_turn};
use crate::EnsureError;
use crate::IntentState;
use crate::launch::inspect_tests::{DockerStub, http};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};

#[tokio::test]
async fn definite_assigned_jit_rejection_releases_then_redelivery_retries() -> Result<(), String> {
    let (scratch, journal) = open("assigned-jit-forbidden").await?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(91, 1);
    // One best-effort engine identity probe per turn; empty ID keeps `None`.
    let docker = DockerStub::open(vec![http(200, r#"{"ID":""}"#), http(200, r#"{"ID":""}"#)])?;
    let mut rejected = Script {
        calls: Vec::new(),
        mode: Mode::JitForbidden,
    };
    let mut workers = Vec::new();
    let result = start_turn(
        &mut rejected,
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

    assert_eq!(result, Err(EnsureError::Forbidden));
    assert_eq!(rejected.calls, ["jit"]);
    assert!(workers.is_empty());
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(0));
    let failed = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].subject, "m91");
    assert_eq!(failed[0].state, IntentState::Failed);
    assert!(failed[0].docker_id.is_none());
    assert!(failed[0].dind_id.is_none());
    assert!(failed[0].worker_volume.is_none());

    let mut recovered = Script {
        calls: Vec::new(),
        mode: Mode::JitForbidden,
    };
    let result = start_turn(
        &mut recovered,
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
    // The dead row still owns its identity (unique index), so the redelivered
    // subject cannot bind a fresh row or retry a definitely-failed mint. The
    // fresh row is held uncertain; no HTTP call is made.
    assert_eq!(result, Err(EnsureError::Uncertain));
    assert!(recovered.calls.is_empty());
    assert!(workers.is_empty());
    // The held uncertain row keeps its permit; the dead row does not.
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(1));
    let docker_requests = docker.finish().await?;
    // Only the two canned engine identity probes; no container inspect calls.
    assert_eq!(docker_requests.len(), 2);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].state, IntentState::Failed);
    assert_eq!(rows[0].subject, rows[1].subject);
    assert_eq!(rows[1].state, IntentState::Uncertain);
    assert!(rows[1].docker_id.is_none());
    assert!(rows[1].dind_id.is_none());
    assert!(rows[1].worker_volume.is_none());
    absent(&scratch.file())
}
