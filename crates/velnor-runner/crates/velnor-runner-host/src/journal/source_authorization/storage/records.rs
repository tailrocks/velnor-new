//! Bounded reads of scoped assignment and source rows.

use crate::error::HostError;
pub(super) async fn repository_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let identity: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    serde_json::from_str::<serde_json::Value>(&identity)
        .map_err(|_| HostError::Journal)?
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)
}

pub(super) async fn created_set_count(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_scale_set_intents AS s JOIN connection_runtime_bindings AS b ON b.connection_id = s.connection_id WHERE s.connection_id = ?1 AND s.binding_digest = ?2 AND s.state = 'created' AND s.ownership = 'created' AND s.remote_id = ?3 AND b.scale_set_id = s.remote_id AND b.scale_set_name = s.remote_name AND b.set_ownership = 'created'",
            (connection_id, binding_digest, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 1)
}
