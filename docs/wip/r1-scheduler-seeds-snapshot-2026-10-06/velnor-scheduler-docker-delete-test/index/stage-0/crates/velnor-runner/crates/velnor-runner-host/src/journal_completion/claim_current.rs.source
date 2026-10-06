//! Fresh claim checks for one completion cleanup effect.

use crate::error::HostError;
use crate::journal::Journal;

use super::claim_sql::{finish_transition, unix_seconds};

impl Journal {
    /// Return whether one cleanup generation still owns an unexpired claim.
    pub(crate) async fn completion_cleanup_claim_current(
        &self,
        id: i64,
        generation: i64,
    ) -> Result<bool, HostError> {
        self.completion_cleanup_claim_current_with_clock(id, generation, unix_seconds)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn completion_cleanup_claim_current_with_clock_for_test(
        &self,
        id: i64,
        generation: i64,
        clock: impl Fn() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        self.completion_cleanup_claim_current_with_clock(id, generation, clock)
            .await
    }

    async fn completion_cleanup_claim_current_with_clock(
        &self,
        id: i64,
        generation: i64,
        clock: impl Fn() -> Result<i64, HostError>,
    ) -> Result<bool, HostError> {
        if id <= 0 || generation <= 0 {
            return Err(HostError::Journal);
        }
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
                    "SELECT lease_until FROM completion_cleanup WHERE intent_id = ?1 AND claim_generation = ?2 AND lease_until > ?3",
                    (id, generation, now),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            match rows.next().await.map_err(|_| HostError::Journal)? {
                Some(row) => row.get(0).map(Some).map_err(|_| HostError::Journal),
                None => Ok(None),
            }
        }
        .await;
        finish_transition(&connection, &result).await?;
        let Some(lease_until) = self.sync_after(result).await? else {
            return Ok(false);
        };
        let now = clock()?;
        if now < 0 {
            return Err(HostError::Journal);
        }
        Ok(now < lease_until)
    }
}
