//! Versioned journal initialization and conservative legacy-row migration.

use std::collections::HashMap;

use crate::error::HostError;

mod connection;
mod image_import;
mod migration;
mod remote;
mod request_ledger;
mod scale_set_create;
mod session;
mod session_refresh;
mod source_authorization;
mod provisioning;
mod storage_budget;

pub(super) const JOURNAL_VERSION: i64 = 16;
const CURRENT_COLUMNS: [ColumnShape; 23] = [
    ColumnShape::new("id", "INTEGER", false, None),
    ColumnShape::new("kind", "TEXT", true, None),
    ColumnShape::new("subject", "TEXT", true, None),
    ColumnShape::new("state", "TEXT", true, None),
    ColumnShape::new("docker_id", "TEXT", false, None),
    ColumnShape::new("github_runner_id", "TEXT", false, None),
    ColumnShape::new("cleanup_proven", "INTEGER", true, Some("0")),
    ColumnShape::new("dind_id", "TEXT", false, None),
    ColumnShape::new("worker_volume", "TEXT", false, None),
    ColumnShape::new("scale_set_id", "INTEGER", false, None),
    ColumnShape::new("runner_request_id", "INTEGER", false, None),
    ColumnShape::new("runner_name", "TEXT", false, None),
    ColumnShape::new("acquire_attempted", "INTEGER", true, Some("0")),
    ColumnShape::new("acquire_resolved", "INTEGER", true, Some("0")),
    ColumnShape::new("acquired", "INTEGER", true, Some("0")),
    ColumnShape::new("jit_requested", "INTEGER", true, Some("0")),
    ColumnShape::new("docker_engine_id", "TEXT", false, None),
    ColumnShape::new("launch_phase", "TEXT", false, None),
    ColumnShape::new("launch_id", "TEXT", false, None),
    ColumnShape::new("assignment_key", "TEXT", false, None),
    ColumnShape::new("seed_generation_id", "TEXT", false, None),
    ColumnShape::new("runner_completed", "INTEGER", true, Some("0")),
    ColumnShape::new("worker_cleanup_proven", "INTEGER", true, Some("0")),
];

#[derive(Clone, Copy)]
struct ColumnShape {
    name: &'static str,
    declared_type: &'static str,
    not_null: bool,
    default_value: Option<&'static str>,
}

impl ColumnShape {
    const fn new(
        name: &'static str,
        declared_type: &'static str,
        not_null: bool,
        default_value: Option<&'static str>,
    ) -> Self {
        Self {
            name,
            declared_type,
            not_null,
            default_value,
        }
    }
}

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
        validate_persisted_schema(conn).await?;
        migration::ensure_completion_objects(conn).await?;
        crate::journal_schema::bootstrap(conn).await?;
        crate::journal_schema::install_revision_triggers(conn).await?;
    } else {
        match version {
            0 => migration::from_version_zero(conn).await?,
            1 => migration::from_version_one(conn).await?,
            2 => migration::from_version_two(conn).await?,
            3 => migration::from_version_three(conn).await?,
            4 => migration::from_version_four(conn).await?,
            5 => migration::from_version_five(conn).await?,
            6 => migration::from_version_six(conn).await?,
            7 => migration::from_version_seven(conn).await?,
            8 => migration::from_version_eight(conn).await?,
            9 => migration::from_version_nine(conn).await?,
            10 => migration::from_version_ten(conn).await?,
            11 => migration::from_version_eleven(conn).await?,
            12 => migration::from_version_twelve(conn).await?,
            13 => migration::from_version_thirteen(conn).await?,
            14 => migration::from_version_fourteen(conn).await?,
            15 => migration::from_version_fifteen(conn).await?,
            _ => return Err(HostError::Journal),
        }
    }
    validate_persisted_schema(conn).await
}

async fn validate_persisted_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_current_schema(conn).await?;
    connection::validate_connection_schema(conn).await?;
    remote::validate(conn).await?;
    image_import::validate(conn).await?;
    scale_set_create::validate(conn).await?;
    source_authorization::validate(conn).await?;
    request_ledger::validate(conn).await?;
    crate::journal_schema::validate_probe_lease(conn).await?;
    crate::journal::capacity_header::validate_connection(conn).await?;
    provisioning::validate(conn).await?;
    session::validate(conn).await?;
    session_refresh::validate(conn).await?;
    storage_budget::validate(conn).await
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
