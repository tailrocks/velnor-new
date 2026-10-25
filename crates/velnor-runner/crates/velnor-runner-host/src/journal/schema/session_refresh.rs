//! Durable state and validation for append-only session-refresh attempts.

use crate::error::HostError;

use super::read_table_columns;

const TABLE: &str = "connection_session_refreshes";
const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS connection_session_refreshes (intent_id INTEGER NOT NULL REFERENCES connection_intents(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), session_id TEXT NOT NULL CHECK (length(CAST(session_id AS BLOB)) BETWEEN 1 AND 256), generation INTEGER NOT NULL CHECK (generation BETWEEN 1 AND 1024), state TEXT NOT NULL CHECK (state IN ('requested', 'succeeded', 'transport_error', 'abandoned_uncertain', 'uncertain')), PRIMARY KEY (intent_id, generation), FOREIGN KEY (connection_id, scale_set_id, session_id) REFERENCES connection_session_creates(connection_id, scale_set_id, session_id))";
const LOOKUP_INDEX_DDL: &str = "CREATE INDEX IF NOT EXISTS connection_session_refresh_lookup ON connection_session_refreshes(connection_id, scale_set_id, intent_id, generation)";

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(TABLE_DDL, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(LOOKUP_INDEX_DDL, ())
        .await
        .map_err(|_| HostError::Journal)?;
    validate(connection).await
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    validate_columns(connection).await?;
    validate_table(connection).await?;
    validate_index(connection).await?;
    validate_rows(connection).await
}

async fn validate_columns(connection: &turso::Connection) -> Result<(), HostError> {
    let expected = [
        ("intent_id", "INTEGER", true, 1),
        ("connection_id", "TEXT", true, 0),
        ("scale_set_id", "INTEGER", true, 0),
        ("session_id", "TEXT", true, 0),
        ("generation", "INTEGER", true, 2),
        ("state", "TEXT", true, 0),
    ];
    let columns = read_table_columns(connection, TABLE).await?;
    if columns.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for (name, kind, not_null, primary_key) in expected {
        let Some(column) = columns.get(name) else {
            return Err(HostError::Journal);
        };
        if !column.declared_type.eq_ignore_ascii_case(kind)
            || column.not_null != not_null
            || column.primary_key_position != primary_key
            || column.default_value.is_some()
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_table(connection: &turso::Connection) -> Result<(), HostError> {
    let actual = object_sql(connection, "table", TABLE).await?;
    if canonical_schema_sql(&actual) != canonical_schema_sql(TABLE_DDL) {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_index(connection: &turso::Connection) -> Result<(), HostError> {
    let actual = object_sql(connection, "index", "connection_session_refresh_lookup").await?;
    if index_columns(connection).await?
        != ["connection_id", "scale_set_id", "intent_id", "generation"]
        || canonical_schema_sql(&actual) != canonical_schema_sql(LOOKUP_INDEX_DDL)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid_binding = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_session_refreshes AS r LEFT JOIN connection_session_creates AS s ON s.intent_id = r.intent_id AND s.connection_id = r.connection_id AND s.scale_set_id = r.scale_set_id AND s.session_id = r.session_id LEFT JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id AND i.scale_set_id = r.scale_set_id AND i.kind = 'session' AND i.subject = r.session_id WHERE s.intent_id IS NULL OR i.id IS NULL OR (r.state = 'requested' AND (s.state != 'bound' OR i.cleanup_proven != 0))",
    )
    .await?;
    let invalid_generations = scalar_count(
        connection,
        "SELECT COUNT(*) FROM (SELECT intent_id FROM connection_session_refreshes GROUP BY intent_id HAVING MIN(generation) != 1 OR MAX(generation) != COUNT(*))",
    )
    .await?;
    let invalid_successors = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_session_refreshes AS old JOIN connection_session_refreshes AS new ON new.intent_id = old.intent_id AND new.generation = old.generation + 1 WHERE old.state NOT IN ('succeeded', 'transport_error', 'abandoned_uncertain')",
    )
    .await?;
    let non_latest_requests = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_session_refreshes AS r WHERE r.state = 'requested' AND r.generation != (SELECT MAX(latest.generation) FROM connection_session_refreshes AS latest WHERE latest.intent_id = r.intent_id)",
    )
    .await?;
    if invalid_binding == 0
        && invalid_generations == 0
        && invalid_successors == 0
        && non_latest_requests == 0
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn object_sql(
    connection: &turso::Connection,
    object_type: &str,
    name: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (object_type, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)
}

async fn index_columns(connection: &turso::Connection) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query("PRAGMA index_info(connection_session_refresh_lookup)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(row.get(2).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}

async fn scalar_count(connection: &turso::Connection, query: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(query, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

fn canonical_schema_sql(sql: &str) -> String {
    let mut normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    for (source, canonical) in [
        ("createtableifnotexists", "createtable"),
        ("createindexifnotexists", "createindex"),
    ] {
        if normalized.starts_with(source) {
            normalized.replace_range(..source.len(), canonical);
            break;
        }
    }
    normalized
}
