//! Docker inspect responses must not turn uncertainty into absence.

use std::future::Future;
use std::time::Duration;

use super::reconcile_worker;
use crate::error::HostError;
use crate::journal::LaunchIdentity;

mod docker_stub;
use docker_stub::{
    DockerStub, delayed_inspect_close, inspect_response, inspect_response_without_body,
    reconcile_responses,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn reconcile_treats_only_confirmed_not_found_as_absence() -> Result<(), String> {
    let identity = identity()?;
    let stub = DockerStub::open(reconcile_responses(
        &identity,
        inspect_response(404, r#"{"message":"missing"}"#, &identity, "runner"),
    ))?;
    let observed = within(reconcile_worker(&stub.docker, &identity, None, None, None)).await?;
    stub.finish().await?;

    assert_eq!(observed, Ok(super::ObservedWorker::default()));
    Ok(())
}

#[tokio::test]
async fn reconcile_rejects_empty_missing_and_malformed_container_ids() -> Result<(), String> {
    let identity = identity()?;
    let cases = [
        (
            inspect_response(
                200,
                r#"{"Id":"","Config":{"Labels":{}}}"#,
                &identity,
                "runner",
            ),
            HostError::Ownership,
        ),
        (
            inspect_response(200, r#"{"Config":{"Labels":{}}}"#, &identity, "runner"),
            HostError::Ownership,
        ),
        (
            inspect_response(
                200,
                r#"{"Id":"not-an-id","Config":{"Labels":{}}}"#,
                &identity,
                "runner",
            ),
            HostError::Ownership,
        ),
        (
            inspect_response(200, "not-json", &identity, "runner"),
            HostError::Docker,
        ),
    ];

    for (response, expected) in cases {
        let stub = DockerStub::open(reconcile_responses(&identity, response))?;
        let result = within(reconcile_worker(&stub.docker, &identity, None, None, None)).await?;
        stub.finish().await?;
        assert_eq!(result, Err(expected));
    }
    Ok(())
}

#[tokio::test]
async fn reconcile_keeps_non_not_found_docker_errors_as_errors() -> Result<(), String> {
    let identity = identity()?;
    for status in [401, 500, 503] {
        let message = format!(r#"{{"message":"private {status} detail"}}"#);
        let response = inspect_response(status, &message, &identity, "runner");
        let stub = DockerStub::open(reconcile_responses(&identity, response))?;
        let result = within(reconcile_worker(&stub.docker, &identity, None, None, None)).await?;
        stub.finish().await?;
        assert_eq!(result, Err(HostError::Docker));
        if format!("{result:?}").contains("private") {
            return Err("Docker response details escaped into the host error".to_owned());
        }
    }
    Ok(())
}

#[tokio::test]
async fn reconcile_keeps_closed_connections_as_errors() -> Result<(), String> {
    let identity = identity()?;
    let stub = DockerStub::open(reconcile_responses(
        &identity,
        inspect_response_without_body(&identity, "runner"),
    ))?;
    let result = within(reconcile_worker(&stub.docker, &identity, None, None, None)).await?;
    stub.finish().await?;

    assert_eq!(result, Err(HostError::Docker));
    Ok(())
}

#[tokio::test]
async fn reconcile_keeps_inspect_timeouts_uncertain() -> Result<(), String> {
    let identity = identity()?;
    let response =
        delayed_inspect_close(&identity, "runner", CLIENT_TIMEOUT + Duration::from_secs(1));
    let stub = DockerStub::open(reconcile_responses(&identity, response))?;
    let result = tokio::time::timeout(
        CLIENT_TIMEOUT + Duration::from_secs(3),
        reconcile_worker(&stub.docker, &identity, None, None, None),
    )
    .await
    .map_err(|_| "reconciliation did not respect its Docker inspect timeout".to_owned())?;
    stub.finish().await?;

    assert_eq!(result, Err(HostError::DockerTimeout));
    Ok(())
}

fn identity() -> Result<LaunchIdentity, String> {
    LaunchIdentity::new(
        "11111111111111111111111111111111",
        1,
        "22222222222222222222222222222222",
        "engine_identity",
    )
    .map_err(|error| error.to_string())
}

async fn within<F: Future>(future: F) -> Result<F::Output, String> {
    tokio::time::timeout(CLIENT_TIMEOUT + Duration::from_secs(3), future)
        .await
        .map_err(|_| "Docker reconciliation timed out".to_owned())
}
