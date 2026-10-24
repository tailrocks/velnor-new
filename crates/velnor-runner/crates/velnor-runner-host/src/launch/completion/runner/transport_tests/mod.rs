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
use support::{
    Fixture, Response, ScriptedTransport, assert_request, assert_runner_lookup, empty_runners,
    runner_body,
};

#[tokio::test]
async fn exact_runner_is_deleted_then_absence_is_recorded() -> Result<(), HostError> {
    let fixture = Fixture::new("runner-delete").await?;
    let (launch, claim) = fixture.claim().await?;
    let mut transport = ScriptedTransport::new([
        Response::json(
            200,
            runner_body(
                launch.identity.runner_id,
                &launch.identity.runner_name,
                launch.identity.scale_set_id,
            ),
        ),
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
    assert_runner_lookup(
        &transport.requests[0],
        Method::Get,
        &launch.identity.runner_name,
    );
    let delete_path = format!(
        "_apis/distributedtask/pools/0/agents/{}",
        launch.identity.runner_id
    );
    assert_request(
        &transport.requests[1],
        Method::Delete,
        &delete_path,
        Some("api-version=6.0-preview"),
    );
    assert_runner_lookup(
        &transport.requests[2],
        Method::Get,
        &launch.identity.runner_name,
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
        runner_body(
            launch.identity.runner_id + 1,
            &launch.identity.runner_name,
            launch.identity.scale_set_id,
        ),
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
    assert_runner_lookup(
        &transport.requests[0],
        Method::Get,
        &launch.identity.runner_name,
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
        runner_body(
            launch.identity.runner_id,
            &launch.identity.runner_name,
            launch.identity.scale_set_id + 1,
        ),
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
        runner_body(
            launch.identity.runner_id,
            "foreign-name",
            launch.identity.scale_set_id,
        ),
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
        Response::json(
            200,
            runner_body(
                launch.identity.runner_id,
                &launch.identity.runner_name,
                launch.identity.scale_set_id,
            ),
        ),
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
        Response::json(
            200,
            runner_body(
                launch.identity.runner_id,
                &launch.identity.runner_name,
                launch.identity.scale_set_id,
            ),
        ),
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
    let delete_path = format!(
        "_apis/distributedtask/pools/0/agents/{}",
        launch.identity.runner_id
    );
    assert_request(
        &first_transport.requests[1],
        Method::Delete,
        &delete_path,
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
