//! The single transactional v39-to-v40 transition.

use crate::error::HostError;

use super::ddl;

pub(super) async fn migrate_v39_to_v40(conn: &turso::Connection) -> Result<(), HostError> {
    super::super::legacy_validation::validate_persisted_schema_v39(conn).await?;
    create_recovery_tables(conn).await?;
    rebuild_jit_table(conn).await?;
    replace_terminal_view(conn).await?;
    crate::journal::schema::post_jit_effect_fences::restore_jit_attached(conn).await?;
    install_guards(conn).await?;
    replace_metadata_claim_guard(conn).await?;
    conn.execute("PRAGMA user_version=40", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::super::validate_persisted_schema(conn).await?;
    Ok(())
}

async fn replace_metadata_claim_guard(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute("DROP TRIGGER metadata_settlement_claim_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = super::guards::metadata_claim_guard_sql_v40()?;
    conn.execute(&sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn create_recovery_tables(conn: &turso::Connection) -> Result<(), HostError> {
    for sql in [ddl::CLAIMS, ddl::PROGRESS, ddl::SETTLEMENTS] {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn rebuild_jit_table(conn: &turso::Connection) -> Result<(), HostError> {
    let old_sql = jit_table_sql(conn).await?;
    let new_sql = replace_once(
        &old_sql,
        "CREATE TABLE connection_runner_jits (",
        "CREATE TABLE connection_runner_jits_v40 (",
    )?;
    let new_sql = replace_once(
        &new_sql,
        "state IN ('claimed', 'uncertain', 'bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed', 'cleaned')",
        "state IN ('claimed', 'uncertain', 'bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed', 'cleaned', 'conflict_retired')",
    )?;
    let new_sql = replace_once(
        &new_sql,
        "CHECK ((state IN ('claimed', 'uncertain') AND runner_id IS NULL AND runner_name IS NULL) OR (state NOT IN ('claimed', 'uncertain') AND runner_id IS NOT NULL AND runner_name IS NOT NULL))",
        "CHECK ((state IN ('claimed', 'uncertain', 'conflict_retired') AND runner_id IS NULL AND runner_name IS NULL) OR (state NOT IN ('claimed', 'uncertain', 'conflict_retired') AND runner_id IS NOT NULL AND runner_name IS NOT NULL))",
    )?;
    let expected = new_sql.replace(
        "CREATE TABLE connection_runner_jits_v40 (",
        "CREATE TABLE connection_runner_jits (",
    );
    if crate::journal::schema::sql_normalization::normalize(&expected)
        != crate::journal::schema::sql_normalization::normalize(ddl::JIT_TABLE)
    {
        return Err(HostError::Journal);
    }
    conn.execute(&new_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(
        "INSERT INTO connection_runner_jits_v40 (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name, started_request_id, completed_request_id) SELECT intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name, started_request_id, completed_request_id FROM connection_runner_jits",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let dependent_triggers = suspend_jit_dependent_triggers(conn).await?;
    conn.execute("DROP TABLE connection_runner_jits", ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(
        "ALTER TABLE connection_runner_jits_v40 RENAME TO connection_runner_jits",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    for (_, index_sql) in ddl::JIT_INDEXES {
        conn.execute(index_sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    restore_jit_dependent_triggers(conn, &dependent_triggers).await?;
    Ok(())
}

// The immediately preceding v39 validator checks these exact source-backed trigger contracts.
// Suspending them only inside the migration transaction avoids a schema-reparse gap while the
// JIT table is rebuilt, then reinstalls the same validated SQL before v40 validation.
const JIT_DEPENDENT_EXTERNAL_TRIGGER_NAMES: [&str; 8] = [
    "execution_proof_approval_jit_guard",
    "execution_proof_insert_jit_guard",
    "execution_publication_context_insert_guard",
    "physical_attempt_jit_conflict_receipt_insert_guard",
    "physical_attempt_outcome_insert_guard",
    "physical_attempt_post_jit_retirement_claim_insert_guard",
    "physical_attempt_post_jit_retirement_insert_guard",
    "reservation_network_transition",
];

#[cfg(test)]
pub(super) const REBUILT_TRIGGER_TABLES: [&str; 7] = [
    "connection_execution_proofs",
    "execution_publication_operations",
    "physical_attempt_jit_conflict_receipts",
    "physical_attempt_outcomes",
    "physical_attempt_post_jit_retirement_claims",
    "physical_attempt_post_jit_retirements",
    "capacity_reservation_networks",
];

const REBUILT_TRIGGER_TABLE_FILTER: &str = "tbl_name IN ('connection_execution_proofs', 'execution_publication_operations', 'physical_attempt_jit_conflict_receipts', 'physical_attempt_outcomes', 'physical_attempt_post_jit_retirement_claims', 'physical_attempt_post_jit_retirements', 'capacity_reservation_networks')";

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredTrigger {
    rowid: i64,
    name: String,
    table_name: String,
    sql: String,
}

async fn suspend_jit_dependent_triggers(
    conn: &turso::Connection,
) -> Result<Vec<StoredTrigger>, HostError> {
    let mut rows = conn
        .query(
            "SELECT name,tbl_name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name!='connection_runner_jits' AND instr(lower(sql),'connection_runner_jits')>0 ORDER BY name",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut jit_dependent = Vec::with_capacity(JIT_DEPENDENT_EXTERNAL_TRIGGER_NAMES.len());
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        jit_dependent.push((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<String>(2).map_err(|_| HostError::Journal)?,
        ));
    }
    if jit_dependent
        .iter()
        .map(|(name, _, _)| name.as_str())
        .ne(JIT_DEPENDENT_EXTERNAL_TRIGGER_NAMES)
    {
        return Err(HostError::Journal);
    }

    let mut rows = conn
        .query(
            &format!(
                "SELECT rowid,name,tbl_name,sql FROM sqlite_schema WHERE type='trigger' AND {REBUILT_TRIGGER_TABLE_FILTER} ORDER BY rowid"
            ),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut triggers = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        triggers.push(StoredTrigger {
            rowid: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
            table_name: row.get(2).map_err(|_| HostError::Journal)?,
            sql: row.get(3).map_err(|_| HostError::Journal)?,
        });
    }
    triggers.sort_by_key(|trigger| trigger.rowid);
    let mut captured_jit_dependent = triggers
        .iter()
        .filter(|trigger| {
            trigger
                .sql
                .to_ascii_lowercase()
                .contains("connection_runner_jits")
        })
        .map(|trigger| {
            (
                trigger.name.clone(),
                trigger.table_name.clone(),
                trigger.sql.clone(),
            )
        })
        .collect::<Vec<_>>();
    captured_jit_dependent.sort_by(|left, right| left.0.cmp(&right.0));
    if captured_jit_dependent != jit_dependent {
        return Err(HostError::Journal);
    }

    for trigger in &triggers {
        conn.execute(&drop_trigger_sql(&trigger.name), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(triggers)
}

async fn restore_jit_dependent_triggers(
    conn: &turso::Connection,
    triggers: &[StoredTrigger],
) -> Result<(), HostError> {
    // Turso puts newly created triggers first. Restoring by original rowid keeps the
    // per-table execution order, including each guard's precedence against its fences.
    for trigger in triggers {
        conn.execute(&trigger.sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

fn drop_trigger_sql(name: &str) -> String {
    format!("DROP TRIGGER \"{}\"", name.replace('"', "\"\""))
}

async fn replace_terminal_view(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute("DROP VIEW physical_attempt_terminal_proofs", ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(ddl::TERMINAL_PROOFS_VIEW, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn install_guards(conn: &turso::Connection) -> Result<(), HostError> {
    for (kind, _name, sql) in super::schema_objects() {
        match kind {
            "table" => {}
            "trigger" => {
                conn.execute(sql, ())
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            _ => return Err(HostError::Journal),
        }
    }
    for table in ddl::TABLE_NAMES {
        for (suffix, action) in [
            ("insert", "AFTER INSERT"),
            ("update", "AFTER UPDATE"),
            ("delete", "AFTER DELETE"),
        ] {
            let sql = super::revision_trigger_sql(table, suffix, action);
            conn.execute(&sql, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn jit_table_sql(conn: &turso::Connection) -> Result<String, HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name='connection_runner_jits'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

fn replace_once(source: &str, old: &str, new: &str) -> Result<String, HostError> {
    if source.matches(old).count() != 1 {
        return Err(HostError::Journal);
    }
    Ok(source.replacen(old, new, 1))
}
