// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use zeroize::Zeroizing;

use crate::config::{ConnectionId, HostConfig, RepositoryConnection};
use crate::error::{HostError, RustRunnerFailure};
use crate::host_controller::connection_pat;

pub struct ConnectionPats(BTreeMap<ConnectionId, Zeroizing<String>>);

impl std::fmt::Debug for ConnectionPats {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ConnectionPats")
            .field(&"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub enum PatSource {
    Keychain,
    Fixed(Zeroizing<Vec<u8>>),
}

impl std::fmt::Debug for PatSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PatSource([REDACTED])")
    }
}

impl PatSource {
    pub(super) async fn load(
        &self,
        config: &HostConfig,
        deadline: tokio::time::Instant,
    ) -> Result<ConnectionPats, HostError> {
        match self {
            Self::Keychain => ConnectionPats::load(config, deadline).await,
            Self::Fixed(secret) => ConnectionPats::from_secrets(
                config,
                config
                    .connections()
                    .iter()
                    .map(|connection| (connection.connection_id.clone(), secret.clone())),
            ),
        }
    }
}

impl ConnectionPats {
    pub fn from_secrets(
        config: &HostConfig,
        secrets: impl IntoIterator<Item = (ConnectionId, Zeroizing<Vec<u8>>)>,
    ) -> Result<Self, HostError> {
        let mut pats = BTreeMap::new();
        for (id, secret) in secrets {
            let pat = connection_pat(&secret)?;
            if config.connection(&id).is_none()
                || pats.insert(id, Zeroizing::new(pat.to_owned())).is_some()
            {
                return Err(HostError::Config);
            }
        }
        if pats.len() != config.connections().len() {
            return Err(HostError::Config);
        }
        Ok(Self(pats))
    }

    pub(crate) async fn load(
        config: &HostConfig,
        deadline: tokio::time::Instant,
    ) -> Result<Self, HostError> {
        let mut secrets = Vec::with_capacity(config.connections().len());
        for connection in config.connections() {
            let secret = connection.credential_ref.load_until(deadline).await?;
            secrets.push((connection.connection_id.clone(), secret));
        }
        Self::from_secrets(config, secrets)
    }

    pub(super) fn get(&self, id: &ConnectionId) -> Option<&str> {
        self.0.get(id).map(|pat| pat.as_str())
    }
}

pub fn load_connection_pat(
    binding: &RepositoryConnection,
    timeout: std::time::Duration,
) -> Result<Zeroizing<String>, HostError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::RustRunner(RustRunnerFailure::RuntimeUnavailable))?;
    let deadline = tokio::time::Instant::now() + timeout;
    let loaded = runtime.block_on(binding.credential_ref.load_until(deadline));
    runtime.shutdown_background();
    let secret = loaded?;
    Ok(Zeroizing::new(connection_pat(&secret)?.to_owned()))
}
