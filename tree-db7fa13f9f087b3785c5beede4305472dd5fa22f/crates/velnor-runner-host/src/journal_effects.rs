//! One-way durable transitions before remote assignment and runner creation.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal_sql::intent_row;
use crate::reconcile::IntentRow;

impl Journal {
    /// Read one durable launch row.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is absent or malformed.
    pub(crate) async fn intent(&self, id: i64) -> Result<IntentRow, HostError> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id, kind, subject, state, docker_id, dind_id, github_runner_id, cleanup_proven, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed FROM intents WHERE id = ?1",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        intent_row(&row)
    }

    /// Claim the only AcquireJobs call for this launch.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is absent or already resolved.
    pub(crate) async fn claim_acquire(&self, id: i64) -> Result<bool, HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE intents SET acquire_attempted = 1 WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0 AND acquire_attempted = 0 AND acquire_resolved = 0",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let result = if changed == 1 { Ok(true) } else { Ok(false) };
        self.sync_after(result).await?;
        if changed == 1 {
            return Ok(true);
        }
        let row = self.intent(id).await?;
        if row.kind == "launch" && row.acquire_attempted && !row.acquire_resolved {
            return Ok(false);
        }
        Err(HostError::Journal)
    }

    /// Store the one response to the claimed AcquireJobs call.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when a conflicting response was stored.
    pub(crate) async fn resolve_acquire(&self, id: i64, acquired: bool) -> Result<(), HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE intents SET acquire_resolved = 1, acquired = ?1 WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0 AND acquire_attempted = 1 AND acquire_resolved = 0",
                (acquired, id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            self.sync_lineage().await?;
        }
        if changed == 1 {
            return Ok(());
        }
        let row = self.intent(id).await?;
        if row.acquire_resolved && row.acquired == acquired {
            return Ok(());
        }
        Err(HostError::Journal)
    }

    /// Claim the only JIT registration call for this launch.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is absent or cleaned.
    pub(crate) async fn claim_jit(&self, id: i64) -> Result<bool, HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE intents SET jit_requested = 1 WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0 AND jit_requested = 0",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        self.sync_after(if changed == 1 { Ok(true) } else { Ok(false) })
            .await?;
        if changed == 1 {
            return Ok(true);
        }
        let row = self.intent(id).await?;
        if row.kind == "launch" && row.jit_requested && !row.cleanup_proven {
            return Ok(false);
        }
        Err(HostError::Journal)
    }
}
