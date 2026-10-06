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

    /// Whether another unproven launch row already holds this identity.
    ///
    /// A redelivered subject whose dead row still owns `(scale_set_id,
    /// runner_name)` cannot bind a fresh row (unique index
    /// `intents_completion_name`). The caller holds instead of failing.
    pub(crate) async fn launch_identity_taken(
        &self,
        scale_set_id: i64,
        runner_name: &str,
        except_id: i64,
    ) -> Result<bool, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id FROM intents WHERE kind = 'launch' AND scale_set_id = ?1 AND runner_name = ?2 AND cleanup_proven = 0 AND id != ?3 LIMIT 1",
                (
                    scale_set_id,
                    runner_name.to_owned(),
                    except_id,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
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
