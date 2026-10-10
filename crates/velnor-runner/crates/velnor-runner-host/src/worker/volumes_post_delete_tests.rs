//! Post-delete settlement holds without remote proof.

use crate::launch::admission;
use crate::{HostError, IntentState, Outcome};

use super::super::remove_worker_volumes;
use super::{DockerStub, WORKER, http, volume_json};

#[tokio::test]
async fn uncertain_volume_holds_without_remote_settlement() -> Result<(), String> {
    let scratch = crate::launch_harness::Scratch::new("volume-post-delete")
        .map_err(|error| error.to_string())?;
    let journal = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let (row, fresh) = journal
        .begin_launch("offer-transport")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_worker_volume(row, WORKER)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let stub = DockerStub::open(Vec::new())?;
    let decision = admission(
        &stub.docker,
        &journal,
        1,
        1,
        0,
        &crate::launch_harness::assigned_wait(1, 1),
    )
    .await;
    let requests = stub.finish().await?;

    assert_eq!(decision, Ok(crate::launch::Admit::Hold));
    assert_eq!(requests, Vec::new());
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Uncertain);
    assert_eq!(rows[0].docker_id, None);
    assert_eq!(rows[0].dind_id, None);
    assert_eq!(rows[0].worker_volume.as_deref(), Some(WORKER));
    assert!(!rows[0].cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn post_delete_non_not_found_returns_docker_error() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(200, &volume_json("wtransport", WORKER, "socket")),
        http(204, ""),
        http(500, r#"{"message":"not absent"}"#),
    ])?;
    let removed = remove_worker_volumes(&stub.docker, WORKER).await;
    let requests = stub.finish().await?;

    assert_eq!(removed, Err(HostError::Docker));
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET "));
    assert!(requests[1].starts_with("DELETE "));
    assert!(requests[2].starts_with("GET "));
    Ok(())
}
