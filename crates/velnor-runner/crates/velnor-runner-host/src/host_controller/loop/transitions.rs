//! Config transition ordering and durable retirement effects.

use std::path::Path;

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    controller_ipc::ControlState,
    error::HostError,
    journal::Journal,
    launch::{SelectedEngineContext, VerifiedWorkerImages},
};

use super::{setup, state::PreparedConnection};

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
        let prepared = setup::prepare_connection(
            self.state,
            self.journal,
            binding,
            config,
            self.selected_engine,
            self.worker_images,
            self.docker,
        )
        .await
        .map_err(|_| HostError::Journal)?;
        self.connection_sessions.push(prepared);
        Ok(())
    }

    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError> {
        self.control_state.replace_config(config)
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
