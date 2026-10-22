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

pub(super) const JOURNAL_VERSION: i64 = 38;
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
        // Repair only the exact v26 attachment/path contracts before
        // provisioning validation requires the current v27 definitions.
        execution_authorization::bootstrap(conn).await?;
        storage_root_backing::upgrade_v27(conn).await?;
        validate_persisted_schema_v19(conn).await?;
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
        post_jit_retirement::validate_v37(conn).await?;
        post_jit_effect_fences::validate(conn).await?;
        pr_source_transition::validate(conn).await?;
    } else {
        driver::migrate_to_current(conn, version).await?;
    }
    validate_persisted_schema(conn).await
}

#[cfg(test)]
#[path = "schema/migration_driver_tests.rs"]
mod migration_driver_tests;

#[cfg(test)]
#[path = "schema/historical_fixture.rs"]
pub(crate) mod historical_fixture;

async fn validate_persisted_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_persisted_schema_v38(conn).await
}

pub(super) async fn validate_persisted_schema_v36(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v35(conn).await?;
    post_jit_effect_fences::validate(conn).await
}

pub(super) async fn validate_persisted_schema_v37(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v32_core(conn).await?;
    storage_network::validate_v33(conn).await?;
    physical_attempts::validate_v35(conn).await?;
    post_jit_effect_fences::validate(conn).await?;
    post_jit_retirement::validate_v37(conn).await
}

pub(super) async fn validate_persisted_schema_v38(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v37(conn).await?;
    pr_source_transition::validate(conn).await?;
    crate::journal::pr_source_transition::validate_persisted_rows(conn).await
}

pub(super) async fn validate_persisted_schema_v33(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v32_core(conn).await?;
    storage_network::validate_v33(conn).await?;
    physical_attempts::validate_v33(conn).await
}

pub(super) async fn validate_persisted_schema_v34(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v32_core(conn).await?;
    storage_network::validate_v33(conn).await?;
    physical_attempts::validate(conn).await
}

pub(super) async fn validate_persisted_schema_v32(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    storage_network::validate(conn).await?;
    validate_persisted_schema_v32_core(conn).await
}

async fn validate_persisted_schema_v32_core(conn: &turso::Connection) -> Result<(), HostError> {
    validate_persisted_schema_v27_core(conn).await?;
    storage_root_backing::validate(conn).await?;
    execution_publication::validate_v30(conn).await?;
    cleanup_observer_witness::validate_current(conn).await?;
    pre_jit_cleanup::validate(conn).await
}

pub(super) async fn validate_persisted_schema_v27(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v27_core(conn).await?;
    storage_network::validate(conn).await?;
    storage_root_backing::validate_v27(conn).await
}

async fn validate_persisted_schema_v27_core(conn: &turso::Connection) -> Result<(), HostError> {
    validate_persisted_schema_v20(conn).await?;
    storage_initialization::validate_v22(conn).await?;
    execution_authorization::validate(conn).await?;
    execution_publication::validate(conn).await?;
    execution_publication::validate_v27(conn).await?;
    storage_handoff::validate(conn).await?;
    Ok(())
}

pub(super) async fn validate_persisted_schema_v20(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v19(conn).await?;
    storage_initialization::validate(conn).await?;
    crate::journal_schema::validate_v20_revision_triggers(conn).await?;
    Ok(())
}

pub(super) async fn validate_persisted_schema_v19(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_persisted_schema_v17(conn).await?;
    doctor_probe::validate(conn).await?;
    request_reservations::validate(conn).await
}

async fn validate_persisted_schema_v17(conn: &turso::Connection) -> Result<(), HostError> {
    let validations = [
        ("current", validate_current_schema(conn).await),
        (
            "connection",
            connection::validate_connection_schema(conn).await,
        ),
        ("remote", remote::validate(conn).await),
        ("image_import", image_import::validate(conn).await),
        ("scale_set_create", scale_set_create::validate(conn).await),
        (
            "source_authorization",
            source_authorization::validate(conn).await,
        ),
        ("request_ledger", request_ledger::validate(conn).await),
        (
            "probe_lease",
            crate::journal_schema::validate_probe_lease(conn).await,
        ),
        (
            "capacity_header",
            crate::journal::capacity_header::validate_connection(conn).await,
        ),
        ("provisioning", provisioning::validate(conn).await),
        ("session", session::validate(conn).await),
        ("session_refresh", session_refresh::validate(conn).await),
        ("storage_budget", storage_budget::validate(conn).await),
    ];
    for (_, result) in validations {
        result?;
    }
    Ok(())
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
