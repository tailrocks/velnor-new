//! Monotonic effect-boundary updates for prepared launches.

use super::Journal;
use crate::error::HostError;
use crate::reconcile::LaunchPhase;

impl Journal {
    /// Persist that a prepared launch reached `phase` before the next effect.
    ///
    /// Legacy rows without a phase are rejected rather than promoted: their
    /// missing field does not prove which effects may already have happened.
    pub(crate) async fn advance_launch_phase(
        &self,
        id: i64,
        phase: LaunchPhase,
    ) -> Result<(), HostError> {
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = update_phase(&conn, id, phase).await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }
}

async fn update_phase(
    conn: &turso::Connection,
    id: i64,
    next: LaunchPhase,
) -> Result<(), HostError> {
    let mut rows = conn
        .query("SELECT launch_phase FROM intents WHERE id = ?1", [id])
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let Some(row) = row else {
        return Err(HostError::Journal);
    };
    let current: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let current = current
        .as_deref()
        .map(LaunchPhase::parse)
        .transpose()?
        .ok_or(HostError::Journal)?;
    if next.rank() < current.rank() {
        return Err(HostError::Journal);
    }
    if next == current {
        return Ok(());
    }
    let changed = conn
        .execute(
            "UPDATE intents SET launch_phase = ?1 WHERE id = ?2 AND launch_phase = ?3",
            (next.as_str().to_owned(), id, current.as_str().to_owned()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
