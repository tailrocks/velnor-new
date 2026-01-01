//! Versioned journal initialization and conservative legacy-row migration.

use std::collections::HashMap;

use crate::error::HostError;

const JOURNAL_VERSION: i64 = 1;
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

struct ColumnInfo {
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
    match journal_version(conn).await? {
        0 => migrate_version_zero(conn).await,
        JOURNAL_VERSION => {
            ensure_completion_objects(conn).await?;
            crate::journal_schema::bootstrap(conn).await?;
            crate::journal_schema::install_revision_triggers(conn).await?;
            validate_current_schema(conn).await
        }
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
    // Extended statements run first so legacy rows are quarantined before the
    // lifecycle flags exist with a default; the single transaction keeps the
    // migration atomic when validation fails below.
    crate::journal_schema::bootstrap(conn).await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_legacy_columns(conn).await?;
    ensure_completion_objects(conn).await?;
    crate::journal_schema::install_revision_triggers(conn).await?;
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
        if !columns.contains_key(column) {
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

async fn ensure_completion_objects(conn: &turso::Connection) -> Result<(), HostError> {
    let columns = read_columns(conn).await?;
    for (column, definition) in [
        ("scale_set_id", "INTEGER"),
        ("runner_request_id", "INTEGER"),
        ("runner_name", "TEXT"),
        ("acquire_attempted", "INTEGER NOT NULL DEFAULT 0"),
        ("acquire_resolved", "INTEGER NOT NULL DEFAULT 0"),
        ("acquired", "INTEGER NOT NULL DEFAULT 0"),
        ("jit_requested", "INTEGER NOT NULL DEFAULT 0"),
        ("docker_engine_id", "TEXT"),
        ("launch_phase", "TEXT"),
    ] {
        if !columns.contains_key(column) {
            conn.execute(
                &format!("ALTER TABLE intents ADD COLUMN {column} {definition}"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        }
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS completion_cleanup (intent_id INTEGER PRIMARY KEY REFERENCES intents(id), scale_set_id INTEGER NOT NULL, runner_request_id INTEGER NOT NULL, runner_id INTEGER NOT NULL, runner_name TEXT NOT NULL, runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1)), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), claim_generation INTEGER NOT NULL DEFAULT 0 CHECK (claim_generation >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0), UNIQUE (scale_set_id, runner_request_id), UNIQUE (scale_set_id, runner_id), UNIQUE (scale_set_id, runner_name))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS completion_inbox (scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), message_id INTEGER NOT NULL CHECK (message_id >= 0), raw_body TEXT NOT NULL CHECK (length(CAST(raw_body AS BLOB)) BETWEEN 1 AND 262144), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), PRIMARY KEY (scale_set_id, message_id))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS launch_recovery (intent_id INTEGER PRIMARY KEY REFERENCES intents(id), generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0), attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0), retry_after INTEGER NOT NULL DEFAULT 0 CHECK (retry_after >= 0), lease_until INTEGER NOT NULL DEFAULT 0 CHECK (lease_until >= 0))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS launch_recovery_due ON launch_recovery(retry_after, lease_until, intent_id)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS guest_probe_owner (id INTEGER PRIMARY KEY CHECK (id = 1), owner_token TEXT NOT NULL CHECK (length(owner_token) = 32 AND owner_token NOT GLOB '*[^0-9a-f]*'))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS completion_inbox_due ON completion_inbox(retry_after, scale_set_id, message_id)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    ensure_runner_absent(conn).await?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS intents_completion_request ON intents(kind, scale_set_id, runner_request_id) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_request_id IS NOT NULL AND cleanup_proven = 0",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS intents_completion_name ON intents(kind, scale_set_id, runner_name) WHERE kind = 'launch' AND scale_set_id IS NOT NULL AND runner_name IS NOT NULL AND cleanup_proven = 0",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn ensure_runner_absent(conn: &turso::Connection) -> Result<(), HostError> {
    let mut has_runner_absent = false;
    let mut rows = conn
        .query("PRAGMA table_info(completion_cleanup)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        has_runner_absent |= name == "runner_absent";
    }
    if !has_runner_absent {
        conn.execute(
            "ALTER TABLE completion_cleanup ADD COLUMN runner_absent INTEGER NOT NULL DEFAULT 0 CHECK (runner_absent IN (0, 1))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
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
    let mut columns = HashMap::new();
    let mut rows = conn
        .query("PRAGMA table_info(intents)", ())
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
