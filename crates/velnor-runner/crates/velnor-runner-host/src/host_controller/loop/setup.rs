//! Connection binding and session setup used by the controller.

use std::{collections::BTreeMap, path::Path, str};

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::{CapacityLimits, EngineContext, Journal, OperationalCapacityLimits},
    launch::{
        CompletionWorker, SelectedEngineContext, VerifiedWorkerImages, open_connection_session,
        retained_tmpfs_memory_bytes, scoped_runtime::RunnerReadyRegistry,
    },
    launch_blocking::ListenFault,
    scale_set::{ConnectionSetup, ensure_connection_scale_set},
    worker::guest_image::write_guest_image,
};

use super::state::{PreparedConnection, SessionCredentials, session_assigned_total};

pub(super) fn discover_retirements(
    state: &Path,
    config: &HostConfig,
    pending: &mut Vec<RepositoryConnection>,
) {
    match crate::host_controller::retirement::removed_bindings(state, config) {
        Ok(bindings) => {
            for binding in bindings {
                if !pending.iter().any(|existing| existing == &binding) {
                    pending.push(binding);
                }
            }
        }
        Err(error) => eprintln!("host controller retirement scan held: {error}"),
    }
}

pub(super) async fn configure_shared_capacity(
    journal: &Journal,
    config: &HostConfig,
) -> Result<(), HostError> {
    let budget = config.resource_budget()?;
    let pair = budget.pair();
    let jobs = config.max_jobs();
    let cpu_millicores = pair
        .cpu_millicores
        .checked_mul(u64::from(jobs))
        .ok_or(HostError::Config)?;
    let memory_bytes = pair
        .memory_bytes
        .checked_add(retained_tmpfs_memory_bytes().ok_or(HostError::Config)?)
        .ok_or(HostError::Config)?
        .checked_mul(u64::from(jobs))
        .ok_or(HostError::Config)?;
    let reserve = budget.admission_reserve();
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs,
                cpu_millicores,
                memory_bytes,
            },
            OperationalCapacityLimits {
                cpu_millicores: reserve.cpu_millicores,
                memory_bytes: reserve.memory_bytes,
            },
        )
        .await
}

pub(super) async fn prepare_active_connections(
    state: &Path,
    journal: &Journal,
    config: &HostConfig,
    selected_engine: &SelectedEngineContext,
    worker_images: &VerifiedWorkerImages,
    docker: &Docker,
) -> Result<Vec<PreparedConnection>, ListenFault> {
    let allocations = journal.header_allocations().await?;
    let cursor = journal.header_fairness_cursor().await?;
    let has_allocations = !allocations.is_empty();
    let mut prepared = Vec::with_capacity(config.connections().len());
    for binding in config.connections() {
        let target = if has_allocations {
            allocations
                .iter()
                .find(|allocation| allocation.connection_id() == &binding.connection_id)
                .map_or(0, crate::journal::HeaderAllocation::target)
        } else {
            requested_header_capacity_after(config, binding, cursor.as_ref())?
        };
        prepared.push(
            prepare_connection(
                state,
                journal,
                binding,
                config,
                selected_engine,
                worker_images,
                docker,
                target,
            )
            .await?,
        );
    }
    Ok(prepared)
}

pub(super) async fn bind_connection(
    journal: &Journal,
    binding: &RepositoryConnection,
    config: &HostConfig,
    selected_engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    let selected = EngineContext {
        engine_id: selected_engine.engine_id().to_owned(),
        endpoint: config.docker.endpoint.clone(),
        platform: config.docker.platform.clone(),
    };
    if !selected_engine.matches_selection(&selected) {
        return Err(HostError::Ownership);
    }
    let scoped = journal.connection_scope(binding).await?;
    scoped
        .bind_engine_context(
            selected_engine.engine_id(),
            &config.docker.endpoint,
            &config.docker.platform,
        )
        .await
}

/// Keep durable identifiers when preparation fails after acquire.
#[must_use]
pub(super) async fn prepare_connection(
    state: &Path,
    journal: &Journal,
    binding: &RepositoryConnection,
    config: &HostConfig,
    selected_engine: &SelectedEngineContext,
    worker_images: &VerifiedWorkerImages,
    docker: &Docker,
    requested_header_capacity: u32,
) -> Result<PreparedConnection, ListenFault> {
    bind_connection(journal, binding, config, selected_engine).await?;
    let credential = crate::keychain::load_secret(
        binding.credential_ref.service(),
        binding.credential_ref.account(),
    )?;
    let pat = connection_pat(&credential)?;
    let github_pat = crate::listen::Secret::new(pat);
    let ConnectionSetup {
        set,
        mut link,
        scoped,
    } = ensure_connection_scale_set(pat, binding, journal).await?;
    drop(credential);
    let stored = scoped.scale_set().await?.ok_or(HostError::Ownership)?;
    let source_binding = scoped.source_authorization_binding().await?;
    if stored.id != set.id
        || stored.name != set.name
        || stored.ownership != set.ownership
        || source_binding.scale_set_id() != set.id
    {
        return Err(HostError::Ownership.into());
    }
    let guest_image = write_guest_image(state, binding, worker_images)?;
    guest_image.runner_bind(binding, worker_images)?;
    let completion = CompletionWorker::start_scoped(
        scoped.clone(),
        docker.clone(),
        link.base(),
        link.token().to_owned(),
        state,
    )?;
    let (session, session_intent, _header_allocation) =
        open_connection_session(&mut link, set.id, &scoped, requested_header_capacity).await?;
    let assigned_total = session_assigned_total(session.statistics());
    Ok(PreparedConnection {
        binding: binding.clone(),
        scope: scoped,
        set_id: set.id,
        session_intent,
        assigned_total,
        poll_cursor: 0,
        poll_retry_at: None,
        guest_image,
        completion: Some(completion),
        credentials: Some(SessionCredentials {
            link,
            session,
            github_pat,
        }),
        runner_ready: RunnerReadyRegistry::new(),
        ready_intents: BTreeMap::new(),
        authorizing: BTreeMap::new(),
        pending_bootstrap: BTreeMap::new(),
        unkeyed_bootstrap: Vec::new(),
        authorized: BTreeMap::new(),
        pending_authorization: BTreeMap::new(),
        unkeyed_authorization: Vec::new(),
        pending_acquired_recovery: BTreeMap::new(),
        unkeyed_acquired_recovery: Vec::new(),
    })
}

#[cfg(test)]
pub(crate) fn requested_header_capacity(
    config: &HostConfig,
    binding: &RepositoryConnection,
) -> Result<u32, HostError> {
    requested_header_capacity_after(config, binding, None)
}

pub(crate) fn requested_header_capacity_after(
    config: &HostConfig,
    binding: &RepositoryConnection,
    cursor: Option<&crate::config::ConnectionId>,
) -> Result<u32, HostError> {
    let connections = config.connections();
    let index = connections
        .iter()
        .position(|candidate| candidate == binding)
        .ok_or(HostError::Config)?;
    let count = u32::try_from(connections.len()).map_err(|_| HostError::Config)?;
    if count == 0 {
        return Err(HostError::Config);
    }
    let total = config.max_jobs();
    if count > total {
        let mut ordered = connections.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| left.connection_id.cmp(&right.connection_id));
        let start = cursor
            .and_then(|cursor| {
                ordered
                    .iter()
                    .position(|candidate| candidate.connection_id > *cursor)
            })
            .unwrap_or(0);
        let id = binding.connection_id.clone();
        let rank = ordered
            .iter()
            .position(|candidate| candidate.connection_id == id)
            .ok_or(HostError::Config)?;
        let distance = (rank + ordered.len() - start) % ordered.len();
        return Ok(u32::from(distance < total as usize));
    }
    let base = total / count;
    let index = u32::try_from(index).map_err(|_| HostError::Config)?;
    let extra = u32::from(index < total % count);
    base.checked_add(extra).ok_or(HostError::Config)
}

pub(super) fn connection_pat(credential: &[u8]) -> Result<&str, HostError> {
    if credential.is_empty()
        || credential.len() > 4096
        || credential.iter().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(HostError::Keychain);
    }
    str::from_utf8(credential).map_err(|_| HostError::Keychain)
}
