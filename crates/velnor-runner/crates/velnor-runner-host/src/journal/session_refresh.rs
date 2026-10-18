//! Startup-only recovery for locally abandoned exact-session refresh attempts.

use crate::error::HostError;
use crate::launch::SessionRefreshStartupAuthority;

use super::Journal;
use super::connection::storage::in_transaction;

impl Journal {
    /// Mark every outstanding request as abandoned by the prior exclusive owner.
    ///
    /// This changes refresh credentials only. It does not settle polls, headers,
    /// reservations, cursors, or any remote effect.
    pub(crate) async fn recover_interrupted_session_refreshes(
        &self,
        authority: SessionRefreshStartupAuthority<'_>,
    ) -> Result<u64, HostError> {
        authority.validate_lock_and_lineage(self).await?;
        let expected_path = authority.canonical_journal_path().to_path_buf();
        let expected_instance = authority.instance_id().to_owned();
        let expected_engine = authority.engine_id().to_owned();
        let expected_revision = authority.revision();
        in_transaction(self, async |connection| {
            if self.path() != expected_path
                || crate::journal_schema::instance_id(connection).await? != expected_instance
                || crate::journal_schema::engine_id(connection).await? != expected_engine
                || crate::journal_schema::revision(connection).await? != expected_revision
            {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_session_refreshes SET state = 'abandoned_uncertain' WHERE state = 'requested'",
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(changed)
        })
        .await
    }
}
