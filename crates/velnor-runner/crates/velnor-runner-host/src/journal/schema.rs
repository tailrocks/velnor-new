//! Versioned journal initialization and conservative legacy-row migration.

use std::collections::HashMap;

use crate::error::HostError;

#[cfg(test)]
mod cleanup_observer_orphan_tests;
mod cleanup_observer_witness;
#[cfg(test)]
mod cleanup_observer_witness_tests;
mod connection;
mod current_columns;
mod doctor_probe;
mod driver;
mod execution_authorization;
mod execution_publication;
mod image_import;
mod jit_conflict_recovery;
mod jit_conflict_settlement;
mod migration;
mod physical_attempts;
#[cfg(test)]
#[path = "schema/physical_attempts_migration_tests.rs"]
mod physical_attempts_migration_tests;
#[cfg(test)]
#[path = "schema/physical_attempts_tests.rs"]
mod physical_attempts_tests;
#[cfg(test)]
#[path = "schema/physical_attempts_v34_migration_tests.rs"]
mod physical_attempts_v34_migration_tests;
mod post_jit_effect_fences;
#[cfg(test)]
#[path = "schema/post_jit_effect_fences_tests.rs"]
mod post_jit_effect_fences_tests;
mod post_jit_retirement;
mod pr_source_transition;
mod pre_jit_cleanup;

pub(in crate::journal) async fn validate_jit_conflict_progress_proofs(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    jit_conflict_settlement::validate_persisted_proofs(conn).await
}
#[cfg(test)]
pub(in crate::journal) mod provisioning;
#[cfg(not(test))]
mod provisioning;
mod remote;
mod request_ledger;
mod request_reservations;
mod scale_set_create;
mod session;
mod session_refresh;
mod source_authorization;
mod sql_normalization;
mod storage_budget;
mod storage_handoff;
mod storage_initialization;
mod storage_network;
mod storage_root_backing;
#[cfg(test)]
#[path = "schema/execution_publication/v30_migration_tests.rs"]
mod v30_migration_tests;
mod validation;
mod worker_health;

pub(super) const JOURNAL_VERSION: i64 = 41;
use current_columns::CURRENT_COLUMNS;
pub(super) use validation::validate_persisted_schema_v35;

pub(super) struct ColumnInfo {
    declared_type: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_position: i64,
}

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = bootstrap_transaction(conn).await;
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

async fn bootstrap_transaction(conn: &turso::Connection) -> Result<(), HostError> {
    let version = journal_version(conn).await?;
    if version == JOURNAL_VERSION {
        Box::pin(bootstrap_current_schema(conn)).await?;
    } else {
        driver::migrate_to_current(conn, version).await?;
    }
    validate_persisted_schema(conn).await
}

async fn bootstrap_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    // Repair only the exact v26 attachment/path contracts before
    // provisioning validation requires the current v27 definitions.
    execution_authorization::bootstrap(conn).await?;
    storage_root_backing::upgrade_v27(conn).await?;
    legacy_validation::validate_persisted_schema_v19_current40(conn).await?;
    migration::ensure_completion_objects(conn).await?;
    crate::journal_schema::bootstrap(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
    storage_initialization::bootstrap(conn).await?;
    crate::journal_schema::install_v20_revision_triggers(conn).await?;
    storage_network::bootstrap(conn).await?;
    storage_initialization::upgrade_v22(conn).await?;
    storage_root_backing::upgrade_v27(conn).await?;
    storage_root_backing::upgrade_v30(conn).await?;
    storage_root_backing::bootstrap(conn).await?;
    execution_authorization::bootstrap(conn).await?;
    execution_publication::bootstrap(conn).await?;
    execution_publication::upgrade_v25(conn).await?;
    execution_publication::upgrade_v30(conn).await?;
    storage_root_backing::upgrade_v31(conn).await?;
    storage_handoff::bootstrap(conn).await?;
    cleanup_observer_witness::bootstrap(conn).await?;
    post_jit_retirement::validate_v40(conn).await?;
    post_jit_effect_fences::validate(conn).await?;
    pr_source_transition::validate(conn).await?;
    Ok(())
}

#[cfg(test)]
#[path = "schema/migration_driver_tests.rs"]
mod migration_driver_tests;

#[cfg(test)]
#[path = "schema/historical_fixture.rs"]
pub(crate) mod historical_fixture;

async fn validate_persisted_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_persisted_schema_v41(conn).await
}

mod legacy_validation;
pub(super) use legacy_validation::{
    validate_persisted_schema_v19, validate_persisted_schema_v20, validate_persisted_schema_v27,
    validate_persisted_schema_v32, validate_persisted_schema_v33, validate_persisted_schema_v34,
    validate_persisted_schema_v36, validate_persisted_schema_v37, validate_persisted_schema_v38,
    validate_persisted_schema_v39,
};
mod validation_v40;
pub(super) use validation_v40::validate_persisted_schema_v40;

async fn validate_persisted_schema_v41(conn: &turso::Connection) -> Result<(), HostError> {
    validate_persisted_schema_v40(conn).await?;
    worker_health::validate(conn).await
}

async fn journal_version(conn: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}

pub(super) async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    for expected in CURRENT_COLUMNS {
        let Some(actual) = columns.get(expected.name) else {
            return Err(HostError::Journal);
        };
        if !actual
            .declared_type
            .trim()
            .eq_ignore_ascii_case(expected.declared_type)
            || actual.not_null != expected.not_null
            || actual.default_value.as_deref() != expected.default_value
        {
            return Err(HostError::Journal);
        }
    }
    if columns
        .values()
        .filter(|column| column.primary_key_position > 0)
        .count()
        != 1
        || columns
            .get("id")
            .is_none_or(|column| column.primary_key_position != 1)
    {
        return Err(HostError::Journal);
    }
    if !has_canonical_id_column(conn).await? || has_primary_key_index(conn).await? {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn has_canonical_id_column(conn: &turso::Connection) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'intents'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let sql = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    let Some(open) = sql.find('(') else {
        return Ok(false);
    };
    let definition = sql[open + 1..]
        .split(',')
        .next()
        .ok_or(HostError::Journal)?;
    let tokens = definition
        .split_ascii_whitespace()
        .map(|token| {
            token
                .trim_matches(['"', '`', '[', ']'])
                .to_ascii_uppercase()
        })
        .collect::<Vec<_>>();
    Ok(tokens == ["ID", "INTEGER", "PRIMARY", "KEY", "AUTOINCREMENT"])
}

async fn has_primary_key_index(conn: &turso::Connection) -> Result<bool, HostError> {
    let mut rows = conn
        .query("PRAGMA index_list(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if row.get::<String>(3).map_err(|_| HostError::Journal)? == "pk" {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn read_columns(conn: &turso::Connection) -> Result<HashMap<String, ColumnInfo>, HostError> {
    read_table_columns(conn, "intents").await
}

pub(super) async fn read_table_columns(
    conn: &turso::Connection,
    table: &str,
) -> Result<HashMap<String, ColumnInfo>, HostError> {
    let mut columns = HashMap::new();
    let mut rows = conn
        .query(&format!("PRAGMA table_info({table})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        columns.insert(
            name,
            ColumnInfo {
                declared_type: row.get::<String>(2).map_err(|_| HostError::Journal)?,
                not_null: row.get::<i64>(3).map_err(|_| HostError::Journal)? != 0,
                default_value: row
                    .get::<Option<String>>(4)
                    .map_err(|_| HostError::Journal)?,
                primary_key_position: row.get::<i64>(5).map_err(|_| HostError::Journal)?,
            },
        );
    }
    Ok(columns)
}
