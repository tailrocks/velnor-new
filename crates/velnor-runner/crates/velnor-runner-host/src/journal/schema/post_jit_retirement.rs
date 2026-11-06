// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

mod ddl;
mod guards;
mod validation;

const TABLES: [&str; 4] = [
    ddl::CLAIMS_DDL,
    ddl::PROGRESS_DDL,
    ddl::REMOTE_ABSENCE_DDL,
    ddl::RETIREMENTS_DDL,
];

const INSERT_GUARDS: [&str; 4] = [
    guards::CLAIM_INSERT_GUARD,
    guards::PROGRESS_INSERT_GUARD,
    guards::REMOTE_ABSENCE_INSERT_GUARD,
    guards::RETIREMENT_INSERT_GUARD,
];

const IMMUTABLE_GUARDS: [&str; 8] = [
    guards::CLAIMS_NO_UPDATE,
    guards::CLAIMS_NO_DELETE,
    guards::PROGRESS_NO_UPDATE,
    guards::PROGRESS_NO_DELETE,
    guards::REMOTE_ABSENCE_NO_UPDATE,
    guards::REMOTE_ABSENCE_NO_DELETE,
    guards::RETIREMENTS_NO_UPDATE,
    guards::RETIREMENTS_NO_DELETE,
];

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for sql in TABLES
        .into_iter()
        .chain([ddl::TERMINAL_PROOFS_VIEW])
        .chain(INSERT_GUARDS)
        .chain(IMMUTABLE_GUARDS)
        .chain([guards::OUTCOME_RETIREMENT_FENCE])
    {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for table in ddl::TABLE_NAMES {
        for (suffix, action) in [
            ("insert", "AFTER INSERT"),
            ("update", "AFTER UPDATE"),
            ("delete", "AFTER DELETE"),
        ] {
            let name = format!("{table}_revision_{suffix}");
            let sql = format!(
                "CREATE TRIGGER IF NOT EXISTS {name} {action} ON {table} BEGIN UPDATE journal_meta SET revision=revision+1 WHERE singleton=1; END"
            );
            conn.execute(&sql, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_objects(conn, guards::CLAIM_INSERT_GUARD).await?;
    validation::validate_revision_triggers(conn).await?;
    validation::validate_rows(conn).await
}

pub(super) async fn validate_v37(conn: &turso::Connection) -> Result<(), HostError> {
    let claim_guard = claim_insert_guard_v37()?;
    validation::validate_objects(conn, &claim_guard).await?;
    validation::validate_revision_triggers(conn).await?;
    validation::validate_rows(conn).await
}

pub(super) async fn validate_v40(conn: &turso::Connection) -> Result<(), HostError> {
    let claim_guard = guards::claim_insert_guard_v37()?;
    validation::validate_objects_with_view(
        conn,
        &claim_guard,
        super::jit_conflict_settlement::terminal_view_sql(),
    )
    .await?;
    validation::validate_revision_triggers(conn).await?;
    validation::validate_rows(conn).await
}

pub(super) fn claim_insert_guard_v37() -> Result<String, HostError> {
    guards::claim_insert_guard_v37()
}
