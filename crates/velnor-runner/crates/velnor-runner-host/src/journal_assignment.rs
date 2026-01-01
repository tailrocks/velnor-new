//! Launch occupancy counts for capacity checks.

use crate::error::HostError;
use crate::journal::Journal;

impl Journal {
    /// Count all launch reservations that lack proven cleanup.
    ///
    /// Pending, uncertain, stopped, and ID-less rows remain occupied. A worker
    /// exit or queue acknowledgement does not release this count.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the count cannot be read.
    pub async fn occupied_launches(&self) -> Result<u32, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT count(*) FROM intents WHERE kind = 'launch' AND cleanup_proven = 0",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let count: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        u32::try_from(count).map_err(|_| HostError::Journal)
    }
}
