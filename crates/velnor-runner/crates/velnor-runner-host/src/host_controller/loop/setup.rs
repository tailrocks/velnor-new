//! Connection binding and session setup used by the controller.

use std::{path::Path, str, sync::Arc};

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::{CapacityLimits, EngineContext, Journal, OperationalCapacityLimits},
    launch::{
        ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages, retained_tmpfs_memory_bytes,
    },
    launch_blocking::ListenFault,
};

use super::state::PreparedConnection;

#[path = "setup/connection.rs"]
mod connection;

pub(super) struct PrepareConnectionRequest<'a> {
    pub(super) state: &'a Path,
    pub(super) journal: &'a Journal,
    pub(super) binding: &'a RepositoryConnection,
    pub(super) config: &'a HostConfig,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) worker_images: &'a VerifiedWorkerImages,
    pub(super) docker: &'a Docker,
    pub(super) options: ConnectionLaunchOptions<'a>,
}

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
    release_read_secret: &Arc<ReleaseReadSecret>,
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
            prepare_connection(PrepareConnectionRequest {
                state,
                journal,
                binding,
                config,
                selected_engine,
                worker_images,
                docker,
                options: ConnectionLaunchOptions {
                    release_read_secret,
                    requested_header_capacity: target,
                },
            })
            .await?,
        );
    }
    Ok(prepared)
}

/// Keep durable identifiers when preparation fails after acquire.
#[must_use = "handle preparation failure to retain durable identifiers"]
pub(super) async fn prepare_connection(
    request: PrepareConnectionRequest<'_>,
) -> Result<PreparedConnection, ListenFault> {
    connection::prepare_connection(request).await
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
        .await?;
    scoped.install_job_authority_policy(config).await
}

pub(super) struct ConnectionLaunchOptions<'a> {
    pub(super) release_read_secret: &'a Arc<ReleaseReadSecret>,
    pub(super) requested_header_capacity: u32,
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

#[cfg(test)]
#[path = "setup/policy_tests.rs"]
mod policy_tests;
