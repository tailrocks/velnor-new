//! A replay-aware launch intent claim.

use super::{Journal, live_id, token_rejected};
use crate::error::HostError;

impl Journal {
    /// Return a fresh launch row only when this subject has no unresolved row.
    ///
    /// The boolean is true for a newly inserted row. A caller must not repeat
    /// acquire or provisioning when an unresolved row already exists.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on invalid input or database failure.
    pub async fn begin_launch(&self, subject: &str) -> Result<(i64, bool), HostError> {
        if token_rejected(subject) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = launch_id(&conn, subject).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }
}

async fn launch_id(conn: &turso::Connection, subject: &str) -> Result<(i64, bool), HostError> {
    if let Some(id) = live_id(conn, "launch", subject).await? {
        return Ok((id, false));
    }
    conn.execute(
        "INSERT INTO intents (kind, subject, state) VALUES ('launch', ?1, 'pending')",
        [subject],
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok((conn.last_insert_rowid(), true))
}
