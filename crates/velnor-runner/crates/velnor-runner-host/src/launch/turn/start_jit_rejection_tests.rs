//! Definite JIT rejection can release only an unacquired population reservation.

use super::start_tests::{ready, zero_assignment_session};
use super::start_turn;
use crate::EnsureError;
use crate::IntentState;
use crate::launch::inspect_tests::DockerStub;
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};

#[tokio::test]
async fn definite_assigned_jit_rejection_releases_then_redelivery_retries() -> Result<(), String> {
    let (scratch, journal) = open("assigned-jit-forbidden").await?;
    let session = zero_assignment_session()?;
    let polled = assigned_wait(91, 1);
    let docker = DockerStub::open(Vec::new())?;
    let mut rejected = Script {
        calls: Vec::new(),
        mode: Mode::JitForbidden,
    };
    let mut workers = Vec::new();
    let result = start_turn(
        &mut rejected,
        &mut workers,
        ready(&session, &polled),
        &journal,
        &docker.docker,
        1,
        false,
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
        ready(&session, &polled),
        &journal,
        &docker.docker,
        1,
        false,
    )
    .await;
    assert_eq!(result, Err(EnsureError::Forbidden));
    assert_eq!(recovered.calls, ["jit"]);
    assert!(workers.is_empty());
    assert_eq!(crate::launch::slot::occupied(&journal).await, Ok(0));
    let docker_requests = docker.finish().await?;
    assert!(docker_requests.is_empty());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].state, IntentState::Failed);
    assert_eq!(rows[0].subject, rows[1].subject);
    assert_eq!(rows[1].state, IntentState::Failed);
    assert!(rows[1].docker_id.is_none());
    assert!(rows[1].dind_id.is_none());
    assert!(rows[1].worker_volume.is_none());
    absent(&scratch.file())
}
