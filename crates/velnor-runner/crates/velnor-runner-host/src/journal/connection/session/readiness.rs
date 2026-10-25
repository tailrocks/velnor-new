//! Readiness checks for the exact durable session behind a successful poll.

use crate::{
    error::HostError,
    journal::{ConnectionJournal, capacity_header},
};

use super::header::active_session_id;
use crate::journal::connection::storage::{has_ambiguous_legacy_work, read_transaction};

impl ConnectionJournal {
    /// Check the durable session and latest credential refresh for one poll receipt.
    pub(crate) async fn readiness_session_matches(
        &self,
        intent_id: i64,
        expected_scale_set_id: i64,
        expected_session_id: &str,
    ) -> Result<bool, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(false);
            }
            let scale_set_id = self.required_set_id(connection).await?;
            if scale_set_id != expected_scale_set_id {
                return Ok(false);
            }
            let session_id = active_session_id(connection, self, scale_set_id, intent_id).await?;
            if session_id != expected_session_id {
                return Ok(false);
            }
            let ledger = capacity_header::load(connection).await?;
            if !ledger.readiness_poll_eligible(self.connection_id(), scale_set_id, &session_id) {
                return Ok(false);
            }
            let mut rows = connection
                .query(
                    "SELECT session_id, state FROM connection_session_refreshes WHERE connection_id = ?1 AND scale_set_id = ?2 AND intent_id = ?3 ORDER BY generation DESC LIMIT 1",
                    (self.connection_id().as_str(), scale_set_id, intent_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(true);
            };
            let refreshed_session = row.get::<String>(0).map_err(|_| HostError::Journal)?;
            let state = row.get::<String>(1).map_err(|_| HostError::Journal)?;
            Ok(refreshed_session == session_id && state == "succeeded")
        })
        .await
    }
}
