// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::ConnectionId;
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::sql::token_rejected;

use super::ConnectionJournal;
use super::storage::{in_transaction, read_transaction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectionScaleSet {
    pub(crate) id: i64,

    pub(crate) name: String,

    pub(crate) ownership: SetOwnership,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceAuthorizationBinding {
    connection_id: ConnectionId,
    scale_set_id: i64,
    binding_digest: String,
}

impl SourceAuthorizationBinding {
    #[must_use]
    pub(crate) fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    #[must_use]
    pub(crate) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EngineContext {
    pub(crate) engine_id: String,

    pub(crate) endpoint: String,

    pub(crate) platform: String,
}

impl ConnectionJournal {
    pub(crate) async fn source_authorization_binding(
        &self,
    ) -> Result<SourceAuthorizationBinding, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT scale_set_id, scale_set_name, set_ownership FROM connection_runtime_bindings WHERE connection_id = ?1",
                    [self.connection_id.as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?;
            let scale_set_id: Option<i64> = row.get(0).map_err(|_| HostError::Journal)?;
            let scale_set_name: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
            let ownership: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
            match (scale_set_id, scale_set_name, ownership) {
                (Some(id), Some(name), Some(ownership)) if id > 0 && !name.is_empty() => {
                    parse_ownership(&ownership)?;
                    Ok(SourceAuthorizationBinding {
                        connection_id: self.connection_id.clone(),
                        scale_set_id: id,
                        binding_digest: self.binding_digest.clone(),
                    })
                }
                _ => Err(HostError::Journal),
            }
        })
        .await
    }

    pub(crate) async fn scale_set(&self) -> Result<Option<ConnectionScaleSet>, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT scale_set_id, scale_set_name, set_ownership FROM connection_runtime_bindings WHERE connection_id = ?1",
                    [self.connection_id.as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let id: Option<i64> = row.get(0).map_err(|_| HostError::Journal)?;
            let name: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
            let ownership: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
            match (id, name, ownership) {
                (None, None, None) => Ok(None),
                (Some(id), Some(name), Some(ownership)) if id > 0 && !name.is_empty() => {
                    Ok(Some(ConnectionScaleSet {
                        id,
                        name,
                        ownership: parse_ownership(&ownership)?,
                    }))
                }
                _ => Err(HostError::Journal),
            }
        })
        .await
    }

    pub(crate) async fn bind_engine_context(
        &self,
        engine_id: &str,
        endpoint: &str,
        platform: &str,
    ) -> Result<(), HostError> {
        if token_rejected(engine_id)
            || token_rejected(endpoint)
            || token_rejected(platform)
            || engine_id.len() > 128
            || endpoint.len() > 4096
            || platform.len() > 64
        {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            if crate::journal_schema::engine_id(connection).await? != engine_id {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT OR IGNORE INTO connection_runtime_bindings (connection_id) VALUES (?1)",
                    [self.connection_id.as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE connection_runtime_bindings SET engine_id = COALESCE(engine_id, ?1), docker_endpoint = COALESCE(docker_endpoint, ?2), platform = COALESCE(platform, ?3) WHERE connection_id = ?4 AND (engine_id IS NULL OR engine_id = ?1) AND (docker_endpoint IS NULL OR docker_endpoint = ?2) AND (platform IS NULL OR platform = ?3)",
                    (engine_id, endpoint, platform, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1
                || same_engine_context(
                    connection,
                    self.connection_id.as_str(),
                    engine_id,
                    endpoint,
                    platform,
                )
                .await?
            {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    pub(crate) async fn engine_context(&self) -> Result<Option<EngineContext>, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT engine_id, docker_endpoint, platform FROM connection_runtime_bindings WHERE connection_id = ?1",
                    [self.connection_id.as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let engine_id: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
            let endpoint: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
            let platform: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
            match (engine_id, endpoint, platform) {
                (None, None, None) => Ok(None),
                (Some(engine_id), Some(endpoint), Some(platform)) => Ok(Some(EngineContext {
                    engine_id,
                    endpoint,
                    platform,
                })),
                _ => Err(HostError::Journal),
            }
        })
        .await
    }
}

async fn same_engine_context(
    connection: &turso::Connection,
    connection_id: &str,
    engine_id: &str,
    endpoint: &str,
    platform: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT engine_id, docker_endpoint, platform FROM connection_runtime_bindings WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored_engine: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_endpoint: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let stored_platform: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    Ok(stored_engine.as_deref() == Some(engine_id)
        && stored_endpoint.as_deref() == Some(endpoint)
        && stored_platform.as_deref() == Some(platform))
}

fn parse_ownership(value: &str) -> Result<SetOwnership, HostError> {
    match value {
        "created" => Ok(SetOwnership::Created),
        "adopted" => Ok(SetOwnership::Adopted),
        _ => Err(HostError::Journal),
    }
}
