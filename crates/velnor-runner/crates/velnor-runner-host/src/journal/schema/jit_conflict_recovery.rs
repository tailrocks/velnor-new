// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

mod ddl;
mod validation;

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for sql in [
        ddl::TABLE,
        ddl::WITNESS_INDEX,
        ddl::INSERT_GUARD,
        ddl::NO_UPDATE,
        ddl::NO_DELETE,
    ] {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (suffix, action) in [
        ("insert", "AFTER INSERT"),
        ("update", "AFTER UPDATE"),
        ("delete", "AFTER DELETE"),
    ] {
        let sql = format!(
            "CREATE TRIGGER IF NOT EXISTS physical_attempt_jit_conflict_receipts_revision_{suffix} {action} ON physical_attempt_jit_conflict_receipts BEGIN UPDATE journal_meta SET revision=revision+1 WHERE singleton=1; END"
        );
        conn.execute(&sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate(conn).await
}
