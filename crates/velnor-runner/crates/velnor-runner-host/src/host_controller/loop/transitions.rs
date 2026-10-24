//! Config transition ordering and durable retirement effects.

use std::{path::Path, sync::Arc};

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    controller_ipc::ControlState,
    error::HostError,
    journal::Journal,
    launch::{ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages},
};

use super::{
    setup,
    state::{PreparedConnection, setup_receipt_inputs},
};

pub(crate) trait TransitionEffects {
    async fn configure_shared_budget(&mut self, config: &HostConfig) -> Result<(), HostError>;
    async fn bind_connection(
        &mut self,
        binding: &RepositoryConnection,
        config: &HostConfig,
    ) -> Result<(), HostError>;
    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError>;
    async fn retire_removed(&mut self, binding: &RepositoryConnection) -> Result<bool, HostError>;
}

/// Apply one config without changing a binding or making a budget per connection.
///
/// Return removed bindings after the new IPC view is active.
pub(crate) async fn apply_config_transition<E: TransitionEffects>(
    previous: &HostConfig,
    next: &HostConfig,
    effects: &mut E,
) -> Result<Vec<RepositoryConnection>, HostError> {
    validate_transition(previous, next)?;
    effects.configure_shared_budget(next).await?;
    for binding in next.connections() {
        effects.bind_connection(binding, next).await?;
    }
    effects.publish_config(next)?;

    Ok(previous
        .connections()
        .iter()
        .filter(|binding| next.connection(&binding.connection_id).is_none())
        .cloned()
        .collect())
}

pub(super) async fn apply_config(
    current: &mut HostConfig,
    pending_retirements: &mut Vec<RepositoryConnection>,
    next: HostConfig,
    effects: &mut JournalEffects<'_>,
) -> bool {
    if let Err(error) = ReleaseReadSecret::load(&next) {
        eprintln!("host controller release-reader reload held: {error}");
        return false;
    }
    match apply_config_transition(current, &next, effects).await {
        Ok(removed) => {
            *current = next;
            pending_retirements.extend(removed);
            true
        }
        Err(error) => {
            eprintln!("host controller config transition held: {error}");
            false
        }
    }
}

pub(crate) async fn retire_pending<E: TransitionEffects>(
    pending: &mut Vec<RepositoryConnection>,
    effects: &mut E,
) -> Result<(), HostError> {
    let mut remaining = Vec::new();
    for binding in pending.iter() {
        if !effects.retire_removed(binding).await? {
            remaining.push(binding.clone());
        }
    }
    *pending = remaining;
    Ok(())
}

fn validate_transition(previous: &HostConfig, next: &HostConfig) -> Result<(), HostError> {
    if previous.docker != next.docker
        || previous.connections().iter().any(|current| {
            next.connection(&current.connection_id)
                .is_some_and(|candidate| candidate != current)
        })
    {
        return Err(HostError::Config);
    }
    Ok(())
}

pub(super) struct JournalEffects<'a> {
    pub(super) state: &'a Path,
    pub(super) journal: &'a Journal,
    pub(super) control_state: &'a ControlState,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) worker_images: &'a VerifiedWorkerImages,
    pub(super) release_read_secret: &'a Arc<ReleaseReadSecret>,
    pub(super) docker: &'a Docker,
    pub(super) connection_sessions: &'a mut Vec<PreparedConnection>,
}

impl TransitionEffects for JournalEffects<'_> {
    async fn configure_shared_budget(&mut self, config: &HostConfig) -> Result<(), HostError> {
        setup::configure_shared_capacity(self.journal, config).await
    }

    async fn bind_connection(
        &mut self,
        binding: &RepositoryConnection,
        config: &HostConfig,
    ) -> Result<(), HostError> {
        if self
            .connection_sessions
            .iter()
            .any(|prepared| prepared.is_for(binding))
        {
            return setup::bind_connection(self.journal, binding, config, self.selected_engine)
                .await;
        }
        let allocations = self.journal.header_allocations().await?;
        let cursor = self.journal.header_fairness_cursor().await?;
        let target = if allocations.is_empty() {
            setup::requested_header_capacity_after(config, binding, cursor.as_ref())?
        } else {
            allocations
                .iter()
                .find(|allocation| allocation.connection_id() == &binding.connection_id)
                .map_or(0, crate::journal::HeaderAllocation::target)
        };
        let prepared = setup::prepare_connection(setup::PrepareConnectionRequest {
            state: self.state,
            journal: self.journal,
            binding,
            config,
            selected_engine: self.selected_engine,
            worker_images: self.worker_images,
            docker: self.docker,
            options: setup::ConnectionLaunchOptions {
                release_read_secret: self.release_read_secret,
                requested_header_capacity: target,
            },
        })
        .await
        .map_err(|_| HostError::Journal)?;
        self.connection_sessions.push(prepared);
        Ok(())
    }

    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError> {
        let receipts = setup_receipt_inputs(
            self.connection_sessions,
            config,
            self.selected_engine.engine_id(),
            self.selected_engine.docker_root_digest(),
        )?;
        self.control_state
            .replace_config_with_setup(config, receipts)
    }

    async fn retire_removed(&mut self, binding: &RepositoryConnection) -> Result<bool, HostError> {
        crate::host_controller::retirement::retire_if_removed(
            self.state,
            self.journal,
            self.control_state,
            binding,
        )
        .await
    }
}
