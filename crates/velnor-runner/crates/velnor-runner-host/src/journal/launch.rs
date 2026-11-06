// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Journal, live_id, token_rejected};
use crate::error::HostError;

impl Journal {
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
