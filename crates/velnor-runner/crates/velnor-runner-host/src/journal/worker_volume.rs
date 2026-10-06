//! Durable worker volume identity, stored before the first Docker mutation.

use super::{Journal, token_rejected, transaction};
use crate::error::HostError;

impl Journal {
    /// Record the worker volume base before creating any worker volume or container.
    ///
    /// A row can bind the same base again during a replay, but cannot be rebound
    /// to another worker's resources.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing, invalid, or already
    /// belongs to another worker volume.
    pub async fn bind_worker_volume(&self, id: i64, volume: &str) -> Result<(), HostError> {
        if token_rejected(volume) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        transaction::with_unique_id(&conn, id, async |conn| {
            let changed = conn
                .execute(
                    "UPDATE intents SET worker_volume = ?1 WHERE id = ?2 AND (worker_volume IS NULL OR worker_volume = ?1)",
                    (volume.to_owned(), id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            match changed {
                1 => Ok(()),
                0 if same_volume(conn, id, volume).await? => Ok(()),
                _ => Err(HostError::Journal),
            }
        })
        .await
    }
}

async fn same_volume(conn: &turso::Connection, id: i64, volume: &str) -> Result<bool, HostError> {
    let mut rows = conn
        .query("SELECT worker_volume FROM intents WHERE id = ?1", [id])
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    Ok(stored.as_deref() == Some(volume))
}
