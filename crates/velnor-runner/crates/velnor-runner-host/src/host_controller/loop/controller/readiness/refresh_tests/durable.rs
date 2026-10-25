//! Test readiness with durable sessions, leases, and a blocked queue poll.

use crate::{config::RepositoryConnection, error::HostError};

use super::super::refresh_with_dependencies;
use super::{on_large_test_stack, poll_receipt, prepare_busy_host, probe_seed};

#[test]
fn controller_refresh_rejects_an_unresolved_session_refresh() -> Result<(), String> {
    on_large_test_stack(controller_refresh_rejects_an_unresolved_session_refresh_async)
}

async fn controller_refresh_rejects_an_unresolved_session_refresh_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host()).await?;
    let receipt = poll_receipt(&host)?;
    let _claim = host
        .fixture
        .scope
        .claim_session_refresh(receipt.session_intent_id(), receipt.session_id())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The durable session refresh was not claimable.".to_owned())?;
    let remote = |_: &RepositoryConnection,
                  _: Option<&velnor_runner_github::RunnerReference>,
                  _: i64| Ok(());
    let result = Box::pin(refresh_with_dependencies(
        &host.state,
        host.docker.client(),
        &host.fixture.journal,
        &host.selected,
        &host.control,
        None,
        &remote,
    ))
    .await;
    let requests = host.docker.finish_observed().await?;
    if !matches!(result, Err(HostError::Ownership)) || !requests.is_empty() {
        return Err(format!(
            "The controller did not reject readiness before the Docker audit: result={result:?}, requests={requests:?}."
        ));
    }
    Ok(())
}

#[test]
fn cached_sample_does_not_hide_an_uncertain_operational_lease() -> Result<(), String> {
    on_large_test_stack(cached_sample_does_not_hide_an_uncertain_operational_lease_async)
}

#[test]
fn durable_session_poll_state_gates_readiness() -> Result<(), String> {
    on_large_test_stack(durable_session_poll_state_gates_readiness_async)
}

async fn durable_session_poll_state_gates_readiness_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host()).await?;
    let receipt = poll_receipt(&host)?;
    let allocation = host
        .fixture
        .scope
        .session_header_allocation(receipt.session_intent_id())
        .await
        .map_err(|error| error.to_string())?;
    let attempt = host
        .fixture
        .scope
        .begin_session_poll(receipt.session_intent_id(), &allocation)
        .await
        .map_err(|error| error.to_string())?;
    if !host
        .fixture
        .scope
        .readiness_session_matches(
            receipt.session_intent_id(),
            receipt.scale_set_id(),
            receipt.session_id(),
        )
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("The exact observed session poll was not eligible for readiness.".to_owned());
    }
    host.fixture
        .scope
        .mark_session_poll_uncertain(receipt.session_intent_id(), attempt)
        .await
        .map_err(|error| error.to_string())?;
    let eligible = host
        .fixture
        .scope
        .readiness_session_matches(
            receipt.session_intent_id(),
            receipt.scale_set_id(),
            receipt.session_id(),
        )
        .await
        .map_err(|error| error.to_string())?;
    let _requests = host.docker.finish_observed().await?;
    if eligible {
        return Err("The uncertain session poll remained eligible for readiness.".to_owned());
    }
    Ok(())
}

async fn cached_sample_does_not_hide_an_uncertain_operational_lease_async() -> Result<(), String> {
    let host = Box::pin(prepare_busy_host()).await?;
    let sample = host
        .control
        .readiness_sample(&host.selected, std::time::Instant::now())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The cached capacity sample is missing.".to_owned())?;
    host.fixture
        .journal
        .prepare_probe(
            probe_seed(&host.fixture.journal, &host.selected)
                .await
                .map_err(|error| error.to_string())?,
        )
        .await
        .map_err(|error| format!("The active probe lease could not be prepared: {error}."))?;
    let usage = host
        .fixture
        .journal
        .readiness_capacity_usage()
        .await
        .map_err(|error| error.to_string())?;
    if usage.valid_for_guest(
        sample.effective_cpu_millicores(),
        sample.memory_total_bytes(),
        sample.memory_available_bytes(),
    ) {
        return Err("The capacity snapshot ignored the unresolved probe lease.".to_owned());
    }
    let remote = |_: &RepositoryConnection,
                  _: Option<&velnor_runner_github::RunnerReference>,
                  _: i64| Ok(());
    let result = Box::pin(refresh_with_dependencies(
        &host.state,
        host.docker.client(),
        &host.fixture.journal,
        &host.selected,
        &host.control,
        None,
        &remote,
    ))
    .await;
    let requests = host.docker.finish_observed().await?;
    result.map_err(|error| error.to_string())?;
    if !requests.is_empty()
        || host
            .fixture
            .journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err(
            "The readiness check changed state or audited Docker while another probe held the lease.".to_owned(),
        );
    }
    Ok(())
}

#[path = "durable/blocked_poll.rs"]
mod blocked_poll;
