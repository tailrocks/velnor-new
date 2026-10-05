//! Versioned journal initialization and conservative legacy-row migration.

use crate::error::HostError;

const JOURNAL_VERSION: i64 = 1;
const CURRENT_COLUMNS: [ColumnSpec; 9] = [
    ColumnSpec {
        name: "id",
        declared_type: "INTEGER",
        not_null: 0,
        default_value: None,
        primary_key: 1,
    },
    ColumnSpec {
        name: "kind",
        declared_type: "TEXT",
        not_null: 1,
        default_value: None,
        primary_key: 0,
    },
    ColumnSpec {
        name: "subject",
        declared_type: "TEXT",
        not_null: 1,
        default_value: None,
        primary_key: 0,
    },
    ColumnSpec {
        name: "state",
        declared_type: "TEXT",
        not_null: 1,
        default_value: None,
        primary_key: 0,
    },
    ColumnSpec {
        name: "docker_id",
        declared_type: "TEXT",
        not_null: 0,
        default_value: None,
        primary_key: 0,
    },
    ColumnSpec {
        name: "github_runner_id",
        declared_type: "TEXT",
        not_null: 0,
        default_value: None,
        primary_key: 0,
    },
    ColumnSpec {
        name: "cleanup_proven",
        declared_type: "INTEGER",
        not_null: 1,
        default_value: Some("0"),
        primary_key: 0,
    },
    ColumnSpec {
        name: "dind_id",
        declared_type: "TEXT",
        not_null: 0,
        default_value: None,
        primary_key: 0,
    },
    ColumnSpec {
        name: "worker_volume",
        declared_type: "TEXT",
        not_null: 0,
        default_value: None,
        primary_key: 0,
    },
];

struct ColumnSpec {
    name: &'static str,
    declared_type: &'static str,
    not_null: i64,
    default_value: Option<&'static str>,
    primary_key: i64,
}

struct ColumnInfo {
    name: String,
    declared_type: String,
    not_null: i64,
    default_value: Option<String>,
    primary_key: i64,
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
    match journal_version(conn).await? {
        0 => migrate_version_zero(conn).await,
        JOURNAL_VERSION => validate_current_schema(conn).await,
        _ => Err(HostError::Journal),
    }
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

async fn migrate_version_zero(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_legacy_columns(conn).await?;
    validate_current_schema(conn).await?;
    conn.execute(
        "UPDATE intents SET state = CASE WHEN state = 'failed' THEN 'uncertain' ELSE state END, cleanup_proven = 0 WHERE kind = 'launch' AND state IN ('failed', 'pending', 'uncertain')",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 1", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn ensure_legacy_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    for (column, definition) in [("dind_id", "TEXT"), ("worker_volume", "TEXT")] {
        if !columns
            .iter()
            .any(|existing| existing.name.eq_ignore_ascii_case(column))
        {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(())
}

async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    if columns.len() != CURRENT_COLUMNS.len() {
        return Err(HostError::Journal);
    }

    for expected in CURRENT_COLUMNS {
        let Some(actual) = columns
            .iter()
            .find(|column| column.name.eq_ignore_ascii_case(expected.name))
        else {
            return Err(HostError::Journal);
        };
        if !actual
            .declared_type
            .trim()
            .eq_ignore_ascii_case(expected.declared_type)
            || actual.not_null != expected.not_null
            || actual.default_value.as_deref() != expected.default_value
            || actual.primary_key != expected.primary_key
        {
            return Err(HostError::Journal);
        }
    }

    if has_separate_primary_key_index(conn).await? {
        return Err(HostError::Journal);
    }

    Ok(())
}

async fn has_separate_primary_key_index(conn: &turso::Connection) -> Result<bool, HostError> {
    let mut rows = conn
        .query("PRAGMA index_list(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if row
            .get::<String>(3)
            .map_err(|_| HostError::Journal)?
            .eq_ignore_ascii_case("pk")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn read_columns(conn: &turso::Connection) -> Result<Vec<ColumnInfo>, HostError> {
    let mut columns = Vec::new();
    let mut rows = conn
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(ColumnInfo {
            name: row.get::<String>(1).map_err(|_| HostError::Journal)?,
            declared_type: row.get::<String>(2).map_err(|_| HostError::Journal)?,
            not_null: row.get::<i64>(3).map_err(|_| HostError::Journal)?,
            default_value: row
                .get::<Option<String>>(4)
                .map_err(|_| HostError::Journal)?,
            primary_key: row.get::<i64>(5).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(columns)
}
