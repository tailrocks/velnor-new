//! Exclusive Scale Set scope checks before the physical runner JIT effect.

use crate::error::HostError;

pub(super) async fn set_is_exclusive(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_runtime_bindings WHERE scale_set_id = ?1 AND connection_id != ?2",
            (scale_set_id, connection_id),
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
    Ok(count == 0)
}
