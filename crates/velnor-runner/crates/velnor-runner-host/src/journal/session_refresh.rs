// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::launch::SessionRefreshStartupAuthority;

use super::Journal;
use super::connection::storage::in_transaction;

mod orphan_recovery;

impl Journal {
    pub(crate) async fn recover_interrupted_host_work(
        &self,
        authority: SessionRefreshStartupAuthority<'_>,
    ) -> Result<(), HostError> {
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
            connection
                .execute(
                    "UPDATE connection_session_refreshes SET state = 'abandoned_uncertain' WHERE state = 'requested'",
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            super::execution_authorization::recover_at_startup(connection).await?;
            orphan_recovery::mark_interrupted_ready_reservations(connection).await?;
            super::doctor_probe::recover_at_startup(connection).await
        })
        .await
    }
}
