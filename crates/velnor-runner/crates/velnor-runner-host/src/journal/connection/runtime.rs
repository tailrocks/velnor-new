//! Immutable per-connection Scale Set and Docker context bindings.

use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::sql::token_rejected;

use super::ConnectionJournal;
use super::storage::{in_transaction, read_transaction};

/// Verified Scale Set identity stored for one repository connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectionScaleSet {
    /// Positive service ID.
    pub(crate) id: i64,
    /// Exact selector name.
    pub(crate) name: String,
    /// Whether this controller created or adopted the Scale Set.
    pub(crate) ownership: SetOwnership,
}

/// Selected local Docker context stored for one repository connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EngineContext {
    /// Immutable Docker engine identity.
    pub(crate) engine_id: String,
    /// Exact selected Docker endpoint.
    pub(crate) endpoint: String,
    /// Exact selected platform.
    pub(crate) platform: String,
}

impl ConnectionJournal {
    /// Bind the verified Scale Set ID, selector name, and ownership.
    pub(crate) async fn bind_scale_set(
        &self,
        id: i64,
        name: &str,
        ownership: SetOwnership,
    ) -> Result<(), HostError> {
        if id <= 0 || token_rejected(name) || name.len() > 128 {
            return Err(HostError::Journal);
        }
        let ownership = ownership_text(ownership);
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            connection
                .execute(
                    "INSERT OR IGNORE INTO connection_runtime_bindings (connection_id) VALUES (?1)",
                    [self.connection_id.as_str()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE connection_runtime_bindings SET scale_set_id = COALESCE(scale_set_id, ?1), scale_set_name = COALESCE(scale_set_name, ?2), set_ownership = COALESCE(set_ownership, ?3) WHERE connection_id = ?4 AND (scale_set_id IS NULL OR scale_set_id = ?1) AND (scale_set_name IS NULL OR scale_set_name = ?2) AND (set_ownership IS NULL OR set_ownership = ?3)",
                    (id, name, ownership, self.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 || same_scale_set(connection, self.connection_id.as_str(), id, name, ownership).await? {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Read this connection's verified Scale Set binding.
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

    /// Bind the selected Docker engine, endpoint, and platform for this connection.
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

    /// Read this connection's selected Docker engine context.
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

async fn same_scale_set(
    connection: &turso::Connection,
    connection_id: &str,
    id: i64,
    name: &str,
    ownership: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id, scale_set_name, set_ownership FROM connection_runtime_bindings WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored_id: Option<i64> = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_name: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let stored_ownership: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    Ok(stored_id == Some(id)
        && stored_name.as_deref() == Some(name)
        && stored_ownership.as_deref() == Some(ownership))
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

fn ownership_text(ownership: SetOwnership) -> &'static str {
    match ownership {
        SetOwnership::Created => "created",
        SetOwnership::Adopted => "adopted",
    }
}

fn parse_ownership(value: &str) -> Result<SetOwnership, HostError> {
    match value {
        "created" => Ok(SetOwnership::Created),
        "adopted" => Ok(SetOwnership::Adopted),
        _ => Err(HostError::Journal),
    }
}
