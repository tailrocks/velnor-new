//! Publish current controller, journal, resource, and owned-runner evidence.

use std::time::Instant;

use crate::{
    controller_ipc::ControlState,
    docker_client::docker_deadline,
    error::HostError,
    journal::Journal,
    launch::{ReleaseReadSecret, SelectedEngineContext},
};
use bollard::Docker;
#[path = "readiness/audit.rs"]
mod audit;
#[cfg(test)]
#[path = "readiness/refresh_tests.rs"]
mod refresh_tests;
#[path = "readiness/worker.rs"]
mod worker;

pub(super) use worker::ReadinessWorker;

async fn refresh(
    state: &std::path::Path,
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    control: &ControlState,
    secret: &ReleaseReadSecret,
) -> Result<(), HostError> {
    Box::pin(refresh_with_dependencies(
        state,
        docker,
        journal,
        selected,
        control,
        Some(secret),
        &audit::audit_remote_runner,
    ))
    .await
}

async fn refresh_with_dependencies(
    state: &std::path::Path,
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    control: &ControlState,
    secret: Option<&ReleaseReadSecret>,
    remote_audit: &audit::RemoteRunnerAudit,
) -> Result<(), HostError> {
    if !control.admission_ready() {
        control.clear_readiness()?;
        return Ok(());
    }
    let Some((config, config_sha256)) = crate::readiness_scan::controller_config(state) else {
        control.clear_readiness()?;
        return Ok(());
    };
    if !control.matches_config(&config)? {
        control.clear_readiness()?;
        return Ok(());
    }
    let Some(polls) = control.complete_session_poll_receipts()? else {
        control.clear_readiness()?;
        return Ok(());
    };
    let Some(sample) = sample_for_refresh(docker, journal, selected, control, secret).await? else {
        return Ok(());
    };
    if !sample.is_recent(Instant::now()) || !sample.matches_engine(selected) {
        control.clear_readiness()?;
        return Ok(());
    }

    let collection_started_at = Instant::now();
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    let Some((capacity, inventory)) = validated_inventory(
        docker,
        journal,
        selected,
        &config,
        &polls,
        &sample,
        remote_audit,
    )
    .await?
    else {
        control.clear_readiness()?;
        return Ok(());
    };
    if !inputs_unchanged(
        state,
        journal,
        control,
        &config,
        &config_sha256,
        &polls,
        &instance_id,
        revision,
    )
    .await?
    {
        control.clear_readiness()?;
        return Ok(());
    }
    control.publish_readiness(
        config_sha256,
        selected.engine_id().to_owned(),
        selected.docker_root_digest().to_owned(),
        instance_id,
        revision,
        sample.generation().to_owned(),
        sample.observed_at(),
        collection_started_at,
        Instant::now(),
        capacity,
        sample.effective_cpu_millicores(),
        sample.memory_total_bytes(),
        sample.memory_available_bytes(),
        inventory,
        polls,
    )
}

async fn validated_inventory(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    config: &crate::config::HostConfig,
    polls: &[crate::controller_ipc::SessionPollReceipt],
    sample: &crate::launch::ReadinessSample,
    remote_audit: &audit::RemoteRunnerAudit,
) -> Result<Option<(crate::journal::ReadinessCapacityUsage, String)>, HostError> {
    if journal.engine_id().await? != selected.engine_id() {
        return Ok(None);
    }
    let capacity = journal.readiness_capacity_usage().await?;
    if !capacity.valid_for_guest(
        sample.effective_cpu_millicores(),
        sample.memory_total_bytes(),
        sample.memory_available_bytes(),
    ) {
        return Ok(None);
    }
    let inventory =
        audit::audit_inventory_with_remote(docker, journal, config, selected, polls, remote_audit)
            .await?;
    #[cfg(test)]
    let error_to_host = |error| {
        audit::trace_docker_failure("info", "selected_engine", &error);
        HostError::Docker
    };
    #[cfg(not(test))]
    let error_to_host = |_| HostError::Docker;
    let live = docker_deadline(docker.info())
        .await?
        .map_err(error_to_host)?;
    let engine_context = crate::journal::EngineContext {
        engine_id: selected.engine_id().to_owned(),
        endpoint: selected.endpoint().to_owned(),
        platform: selected.platform().to_owned(),
    };
    let live_selected = SelectedEngineContext::capture(&engine_context, &live)?;
    if live_selected != *selected {
        return Ok(None);
    }
    Ok(Some((capacity, inventory)))
}

async fn sample_for_refresh(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    control: &ControlState,
    secret: Option<&ReleaseReadSecret>,
) -> Result<Option<crate::launch::ReadinessSample>, HostError> {
    if let Some(sample) = control.readiness_sample(selected, Instant::now())? {
        return Ok(Some(sample));
    }
    let Some(secret) = secret else {
        control.clear_readiness()?;
        return Ok(None);
    };
    let Some(sample) = Box::pin(crate::launch::collect_readiness_sample(
        docker, journal, secret,
    ))
    .await?
    else {
        control.clear_readiness()?;
        return Ok(None);
    };
    control.cache_readiness_sample(sample.clone())?;
    Ok(Some(sample))
}

async fn inputs_unchanged(
    state: &std::path::Path,
    journal: &Journal,
    control: &ControlState,
    config: &crate::config::HostConfig,
    config_sha256: &str,
    polls: &[crate::controller_ipc::SessionPollReceipt],
    instance_id: &str,
    revision: u64,
) -> Result<bool, HostError> {
    let config_matches = crate::readiness_scan::controller_config(state)
        .as_ref()
        .is_some_and(|(current, hash)| current == config && hash == config_sha256);
    Ok(config_matches
        && control.matches_config(config)?
        && control.complete_session_poll_receipts()?.as_deref() == Some(polls)
        && journal.instance_id().await? == instance_id
        && journal.revision().await? == revision)
}
