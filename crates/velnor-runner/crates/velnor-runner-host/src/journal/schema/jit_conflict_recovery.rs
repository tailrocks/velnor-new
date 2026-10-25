//! Durable, exact HTTP 409 receipts for acquired-request JIT attempts.

use crate::error::HostError;

mod ddl;
mod validation;

#[cfg(test)]
#[path = "jit_conflict_recovery/tests.rs"]
mod tests;

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

#[cfg(test)]
pub(super) async fn remove_receipt_schema_for_migration_test(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    for trigger in [
        "physical_attempt_jit_conflict_receipt_insert_guard",
        "physical_attempt_jit_conflict_receipt_no_update",
        "physical_attempt_jit_conflict_receipt_no_delete",
        "physical_attempt_jit_conflict_receipts_revision_insert",
        "physical_attempt_jit_conflict_receipts_revision_update",
        "physical_attempt_jit_conflict_receipts_revision_delete",
    ] {
        conn.execute(&format!("DROP TRIGGER {trigger}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute(
        "DROP INDEX physical_attempt_jit_conflict_receipts_witness",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute("DROP TABLE physical_attempt_jit_conflict_receipts", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
