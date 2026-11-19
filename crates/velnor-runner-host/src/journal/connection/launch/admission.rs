// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::ConnectionJournal;

use crate::error::HostError;

impl ConnectionJournal {
    pub(in crate::journal::connection) async fn required_set_id(
        &self,
        connection: &turso::Connection,
    ) -> Result<i64, HostError> {
        let mut rows = connection
            .query(
                "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
                [self.connection_id.as_str()],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<Option<i64>>(0)
            .map_err(|_| HostError::Journal)?
            .filter(|id| *id > 0)
            .ok_or(HostError::Journal)
    }
}
