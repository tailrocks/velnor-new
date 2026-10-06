//! Per-intent authorization for one bounded completion cleanup effect.

use std::future::Future;

use crate::error::HostError;
use crate::journal::Journal;
use tokio::sync::OwnedMutexGuard;

use super::claim_sql::unix_seconds;

impl Journal {
    /// Run one bounded cleanup effect only while this generation owns a live claim.
    ///
    /// The per-intent guard also excludes a new claim until this one effect returns.
    /// The factory runs only after the durable claim check succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the claim cannot be checked or the effect fails.
    pub(crate) async fn run_completion_cleanup_effect<T, F, Fut>(
        &self,
        id: i64,
        generation: i64,
        effect: F,
    ) -> Result<Option<T>, HostError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, HostError>>,
    {
        let Some(_intent) = self
            .completion_cleanup_effect_guard(id, generation, unix_seconds)
            .await?
        else {
            return Ok(None);
        };
        effect().await.map(Some)
    }

    #[cfg(test)]
    pub(crate) async fn run_completion_cleanup_effect_with_clock_for_test<T, F, Fut>(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
        effect: F,
    ) -> Result<Option<T>, HostError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, HostError>>,
    {
        let Some(_intent) = self
            .completion_cleanup_effect_guard(id, generation, clock)
            .await?
        else {
            return Ok(None);
        };
        effect().await.map(Some)
    }

    async fn completion_cleanup_effect_guard(
        &self,
        id: i64,
        generation: i64,
        clock: impl FnOnce() -> Result<i64, HostError>,
    ) -> Result<Option<OwnedMutexGuard<()>>, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
        let intent = self.completion_lock(id)?.lock_owned().await;
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let now = clock()?;
            if now < 0 {
                return Err(HostError::Journal);
            }
            let mut rows = connection
                .query(
                    "SELECT 1 FROM completion_cleanup WHERE intent_id = ?1 AND claim_generation = ?2 AND lease_until > ?3",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .is_some())
        }
        .await;
        let ended = if result.is_ok() {
            connection.execute("COMMIT", ()).await
        } else {
            connection.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        if result? { Ok(Some(intent)) } else { Ok(None) }
    }
}
