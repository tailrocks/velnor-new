//! Docker inspect errors must stop capacity and reconcile decisions.

use std::future::Future;
use std::time::Duration;

use crate::launch::gate;
use crate::launch_harness::Scratch;
use crate::{EnsureError, IntentState, Journal, Outcome};

mod docker_stub;
pub(crate) use self::docker_stub::DockerStub;
pub(crate) use self::docker_stub::http;
pub(crate) use self::docker_stub::raw_http;
pub(crate) use self::docker_stub::threaded::BackgroundDockerStub;
pub(in crate::launch) use self::docker_stub::{DockerResponse, closed, delayed_http};

const TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn non_not_found_inspect_error_blocks_startup_reconciliation() -> Result<(), String> {
    let (scratch, journal) = journal("inspect-error").await?;
    let row_id = launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(500, r#"{"message":"private runner-id detail"}"#)])?;
    let reconcile = within(
        gate::reconcile_gate(&journal, &stub.docker),
        "reconcile probe",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(
        reconcile,
        Err(EnsureError::Unexpected {
            status: 500,
            step: "docker inspect",
        })
    );
    let errors = format!("{reconcile:?}");
    if errors.contains("runner-id") || errors.contains("private runner-id detail") {
        return Err("inspect error exposed Docker response data".to_owned());
    }
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].id, row_id);
    assert_eq!(after[0].state, IntentState::Pending);
    assert_eq!(after[0].docker_id.as_deref(), Some("runner-id"));
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn failed_done_row_inspect_cannot_advertise_capacity() -> Result<(), String> {
    let (scratch, journal) = journal("done-inspect-error").await?;
    let row_id = launch_row(&journal).await?;
    journal
        .finish(row_id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(500, r#"{"message":"private runner-id detail"}"#)])?;

    let decision = within(
        gate::reconcile_gate(&journal, &stub.docker),
        "reconcile probe",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(
        decision,
        Err(EnsureError::Unexpected {
            status: 500,
            step: "docker inspect",
        })
    );
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].state, IntentState::Done);
    assert_eq!(after[0].docker_id.as_deref(), Some("runner-id"));
    let result = format!("{decision:?}");
    if result.contains("runner-id") || result.contains("private runner-id detail") {
        return Err("reconcile error exposed Docker response data".to_owned());
    }
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn malformed_container_state_cannot_reconcile_or_leak_response_data() -> Result<(), String> {
    let (scratch, journal) = journal("inspect-states").await?;
    launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![http(200, "{}")])?;

    let reconcile = within(
        gate::reconcile_gate(&journal, &stub.docker),
        "reconcile probe",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(
        reconcile,
        Err(EnsureError::Unexpected {
            status: 200,
            step: "docker inspect",
        })
    );
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    assert_eq!(after[0].state, IntentState::Pending);
    no_response_body_in_journal(&scratch.file())
}

#[tokio::test]
async fn closed_docker_connection_blocks_startup_reconciliation() -> Result<(), String> {
    let (scratch, journal) = journal("inspect-transport").await?;
    launch_row(&journal).await?;
    let before = journal.rows().await.map_err(|error| error.to_string())?;
    let stub = DockerStub::open(vec![closed()])?;

    let actual = within(
        gate::reconcile_gate(&journal, &stub.docker),
        "reconcile probe",
    )
    .await?;
    stub.finish().await?;

    assert_eq!(
        actual,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "docker inspect",
        })
    );
    assert_eq!(journal.occupied_launches().await, Ok(1));
    let after = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(after, before);
    no_response_body_in_journal(&scratch.file())
}

pub(super) async fn journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}

pub(super) async fn launch_row(journal: &Journal) -> Result<i64, String> {
    launch_row_for_id(journal, "job", "runner-id").await
}

pub(super) async fn launch_row_for_id(
    journal: &Journal,
    subject: &str,
    docker_id: &str,
) -> Result<i64, String> {
    let row_id = journal
        .begin("launch", subject)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(row_id, Some(docker_id), None)
        .await
        .map_err(|error| error.to_string())?;
    Ok(row_id)
}

pub(super) async fn within<F: Future>(future: F, label: &str) -> Result<F::Output, String> {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .map_err(|_| format!("{label} timed out"))
}

pub(super) fn no_response_body_in_journal(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    if text.contains("private runner-id detail") {
        return Err("journal stored Docker response data".to_owned());
    }
    Ok(())
}
