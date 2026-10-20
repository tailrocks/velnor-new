//! Runner cleanup transport effects and retry behavior.

use std::sync::atomic::AtomicBool;

use velnor_runner_github::TransportFail;
use velnor_runner_github::session::Method;

use super::super::{EffectBudget, Failure};
use super::reconcile_with_transport;
use crate::error::HostError;
use crate::journal::{CleanupClaim, CompletedLaunch};

#[path = "support.rs"]
mod support;
use support::{Fixture, Response, ScriptedTransport, assert_request, empty_runners, runner_body};

const SCALE_SET_ID: i64 = 77;
const REQUEST_ID: i64 = 61;
const RUNNER_ID: i64 = 10_061;
const RUNNER_NAME: &str = "v61";

#[tokio::test]
async fn exact_runner_is_deleted_then_absence_is_recorded() -> Result<(), HostError> {
    let fixture = Fixture::new("runner-delete").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut transport = ScriptedTransport::new([
        Response::json(200, runner_body(RUNNER_ID, RUNNER_NAME, SCALE_SET_ID)),
        Response::status(204),
        Response::json(200, empty_runners()),
    ]);
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);

    reconcile_with_transport(
        &fixture.completion,
        &mut transport,
        &fixture.admin,
        &launch,
        claim,
        &mut budget,
    )
    .await
    .map_err(|failure| failure.error)?;

    assert_eq!(transport.requests.len(), 3);
    assert_request(
        &transport.requests[0],
        Method::Get,
        "_apis/distributedtask/pools/0/agents",
        Some("agentName=v61&api-version=6.0-preview"),
    );
    assert_request(
        &transport.requests[1],
        Method::Delete,
        "_apis/distributedtask/pools/0/agents/10061",
        Some("api-version=6.0-preview"),
    );
    assert_request(
        &transport.requests[2],
        Method::Get,
        "_apis/distributedtask/pools/0/agents",
        Some("agentName=v61&api-version=6.0-preview"),
    );
    assert!(transport.responses.is_empty());
    let current = fixture.due(launch.intent.id).await?;
    assert!(current.identity.runner_absent);
    assert!(!current.intent.cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn foreign_runner_identity_blocks_delete_and_keeps_cleanup_due() -> Result<(), HostError> {
    let fixture = Fixture::new("runner-foreign").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut transport = ScriptedTransport::new([Response::json(
        200,
        runner_body(RUNNER_ID + 1, RUNNER_NAME, SCALE_SET_ID),
    )]);
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);

    let failure = reconcile_with_transport(
        &fixture.completion,
        &mut transport,
        &fixture.admin,
        &launch,
        claim,
        &mut budget,
    )
    .await
    .expect_err("foreign identity must stop deletion");

    assert_eq!(failure, Failure::not_proven("runner identity"));
    assert_eq!(transport.requests.len(), 1);
    assert_request(
        &transport.requests[0],
        Method::Get,
        "_apis/distributedtask/pools/0/agents",
        Some("agentName=v61&api-version=6.0-preview"),
    );
    let current = fixture.due(launch.intent.id).await?;
    assert!(!current.identity.runner_absent);
    assert!(!current.intent.cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn foreign_scale_set_runner_blocks_delete_and_keeps_cleanup_due() -> Result<(), HostError> {
    let fixture = Fixture::new("runner-foreign-set").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut transport = ScriptedTransport::new([Response::json(
        200,
        runner_body(RUNNER_ID, RUNNER_NAME, SCALE_SET_ID + 1),
    )]);
    let failure = run_reconcile(&fixture, &launch, claim, &mut transport)
        .await
        .expect_err("foreign Scale Set identity must stop deletion");

    assert_eq!(failure, Failure::not_proven("runner identity"));
    assert_eq!(transport.requests.len(), 1);
    assert_cleanup_unproven(&fixture, launch.intent.id).await
}

#[tokio::test]
async fn wrong_runner_name_is_rejected_by_lookup_and_keeps_cleanup_due() -> Result<(), HostError> {
    let fixture = Fixture::new("runner-wrong-name").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut transport = ScriptedTransport::new([Response::json(
        200,
        runner_body(RUNNER_ID, "foreign-name", SCALE_SET_ID),
    )]);
    let failure = run_reconcile(&fixture, &launch, claim, &mut transport)
        .await
        .expect_err("wrong exact name must stop deletion");

    assert_eq!(failure.phase, "runner lookup");
    assert_eq!(transport.requests.len(), 1);
    assert_cleanup_unproven(&fixture, launch.intent.id).await
}

#[tokio::test]
async fn delete_timeout_keeps_cleanup_due_and_retry_proves_remote_absence() -> Result<(), HostError>
{
    let fixture = Fixture::new("runner-delete-timeout").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut first_transport = ScriptedTransport::new([
        Response::json(200, runner_body(RUNNER_ID, RUNNER_NAME, SCALE_SET_ID)),
        Response::Failure(TransportFail::Timeout),
    ]);
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);

    let failure = reconcile_with_transport(
        &fixture.completion,
        &mut first_transport,
        &fixture.admin,
        &launch,
        claim,
        &mut budget,
    )
    .await
    .expect_err("delete timeout leaves the remote result uncertain");

    assert_eq!(
        failure,
        Failure::request("runner delete", HostError::Endpoint)
    );
    assert_eq!(first_transport.requests.len(), 2);
    assert_cleanup_unproven(&fixture, launch.intent.id).await?;
    let retry = fixture.retry_claim(launch.intent.id, claim).await?;
    let mut retry_transport = ScriptedTransport::new([
        Response::json(200, empty_runners()),
        Response::json(200, empty_runners()),
    ]);
    let mut retry_budget = EffectBudget::new(&stopping);
    reconcile_with_transport(
        &fixture.completion,
        &mut retry_transport,
        &fixture.admin,
        &launch,
        retry,
        &mut retry_budget,
    )
    .await
    .map_err(|failure| failure.error)?;
    assert_eq!(retry_transport.requests.len(), 2);
    assert!(
        retry_transport
            .requests
            .iter()
            .all(|request| request.method == Method::Get)
    );
    assert!(fixture.due(launch.intent.id).await?.identity.runner_absent);
    Ok(())
}

#[tokio::test]
async fn uncertain_absence_keeps_cleanup_due_until_retry_proves_absence() -> Result<(), HostError> {
    let fixture = Fixture::new("runner-absence-timeout").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut first_transport = ScriptedTransport::new([
        Response::json(200, runner_body(RUNNER_ID, RUNNER_NAME, SCALE_SET_ID)),
        Response::status(204),
        Response::Failure(TransportFail::Reset),
    ]);
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);

    let failure = reconcile_with_transport(
        &fixture.completion,
        &mut first_transport,
        &fixture.admin,
        &launch,
        claim,
        &mut budget,
    )
    .await
    .expect_err("absence lookup reset leaves deletion result uncertain");

    assert_eq!(
        failure,
        Failure::request("runner absence lookup", HostError::Endpoint)
    );
    assert_eq!(first_transport.requests.len(), 3);
    assert_request(
        &first_transport.requests[1],
        Method::Delete,
        "_apis/distributedtask/pools/0/agents/10061",
        Some("api-version=6.0-preview"),
    );
    assert_cleanup_unproven(&fixture, launch.intent.id).await?;
    let retry = fixture.retry_claim(launch.intent.id, claim).await?;
    let mut retry_transport = ScriptedTransport::new([
        Response::json(200, empty_runners()),
        Response::json(200, empty_runners()),
    ]);
    let mut retry_budget = EffectBudget::new(&stopping);
    reconcile_with_transport(
        &fixture.completion,
        &mut retry_transport,
        &fixture.admin,
        &launch,
        retry,
        &mut retry_budget,
    )
    .await
    .map_err(|failure| failure.error)?;
    assert_eq!(retry_transport.requests.len(), 2);
    assert!(
        retry_transport
            .requests
            .iter()
            .all(|request| request.method == Method::Get)
    );
    assert!(fixture.due(launch.intent.id).await?.identity.runner_absent);
    Ok(())
}

async fn assert_cleanup_unproven(fixture: &Fixture, id: i64) -> Result<(), HostError> {
    let current = fixture.due(id).await?;
    assert!(!current.identity.runner_absent);
    assert!(!current.intent.cleanup_proven);
    Ok(())
}

async fn run_reconcile(
    fixture: &Fixture,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    transport: &mut ScriptedTransport,
) -> Result<(), Failure> {
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    reconcile_with_transport(
        &fixture.completion,
        transport,
        &fixture.admin,
        launch,
        claim,
        &mut budget,
    )
    .await
}
