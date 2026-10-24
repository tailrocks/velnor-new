//! Durable session-create records.

use crate::error::HostError;

use super::read_table_columns;

const TABLE: &str = "connection_session_creates";
const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS connection_session_creates (connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), local_nonce TEXT NOT NULL CHECK (length(local_nonce) = 32 AND local_nonce NOT GLOB '*[^0-9a-f]*'), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'bound', 'retired')), session_id TEXT CHECK (session_id IS NULL OR length(CAST(session_id AS BLOB)) BETWEEN 1 AND 256), intent_id INTEGER UNIQUE REFERENCES connection_intents(id), PRIMARY KEY (connection_id, scale_set_id, local_nonce), UNIQUE (connection_id, scale_set_id, session_id), CHECK ((state IN ('prepared', 'requested', 'uncertain') AND session_id IS NULL AND intent_id IS NULL) OR (state IN ('bound', 'retired') AND session_id IS NOT NULL AND intent_id IS NOT NULL)))";
const ACTIVE_INDEX_DDL: &str = "CREATE UNIQUE INDEX IF NOT EXISTS connection_session_create_active ON connection_session_creates(connection_id, scale_set_id) WHERE state IN ('prepared', 'requested', 'uncertain', 'bound')";
const STATE_INDEX_DDL: &str = "CREATE INDEX IF NOT EXISTS connection_session_create_state ON connection_session_creates(state, connection_id, scale_set_id)";

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(TABLE_DDL, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for statement in [ACTIVE_INDEX_DDL, STATE_INDEX_DDL] {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate(connection).await
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    validate_columns(connection).await?;
    validate_table(connection).await?;
    validate_indexes(connection).await?;
    validate_rows(connection).await
}

async fn validate_columns(connection: &turso::Connection) -> Result<(), HostError> {
    let expected = [
        ("connection_id", "TEXT", true, 1),
        ("scale_set_id", "INTEGER", true, 2),
        ("local_nonce", "TEXT", true, 3),
        ("state", "TEXT", true, 0),
        ("session_id", "TEXT", false, 0),
        ("intent_id", "INTEGER", false, 0),
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
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [TABLE],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if canonical_schema_sql(&actual) != canonical_schema_sql(TABLE_DDL) {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_indexes(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA index_list(connection_session_creates)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut primary = false;
    let mut unique_session = false;
    let mut unique_active = false;
    let mut state = false;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
        if unique && name.starts_with("sqlite_autoindex_connection_session_creates") {
            let columns = index_columns(connection, &name).await?;
            primary |= columns == ["connection_id", "scale_set_id", "local_nonce"];
            if columns == ["connection_id", "scale_set_id", "session_id"] {
                unique_session = true;
            }
        }
        if name == "connection_session_create_active" {
            let sql = index_sql(connection, &name).await?;
            unique_active = unique
                && index_columns(connection, &name).await? == ["connection_id", "scale_set_id"]
                && sql == canonical_schema_sql(ACTIVE_INDEX_DDL);
        }
        if name == "connection_session_create_state" {
            state = !unique
                && index_columns(connection, &name).await?
                    == ["state", "connection_id", "scale_set_id"]
                && index_sql(connection, &name).await? == canonical_schema_sql(STATE_INDEX_DDL);
        }
    }
    if primary && unique_session && unique_active && state {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_session_creates AS s WHERE (s.state IN ('prepared', 'requested', 'uncertain') AND (s.session_id IS NOT NULL OR s.intent_id IS NOT NULL)) OR (s.state IN ('bound', 'retired') AND (SELECT COUNT(*) FROM connection_intents AS i WHERE i.id = s.intent_id AND i.connection_id = s.connection_id AND i.scale_set_id = s.scale_set_id AND i.kind = 'session' AND i.subject = s.session_id AND i.cleanup_proven = CASE WHEN s.state = 'retired' THEN 1 ELSE 0 END) != 1)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn index_columns(
    connection: &turso::Connection,
    index: &str,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(&format!("PRAGMA index_info({index})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(row.get(2).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}

async fn index_sql(connection: &turso::Connection, index: &str) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'index' AND name = ?1",
            [index],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(canonical_schema_sql(&sql))
}

fn canonical_schema_sql(sql: &str) -> String {
    let mut normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    for (source, canonical) in [
        ("createtableifnotexists", "createtable"),
        ("createuniqueindexifnotexists", "createuniqueindex"),
        ("createindexifnotexists", "createindex"),
    ] {
        if normalized.starts_with(source) {
            normalized.replace_range(..source.len(), canonical);
            break;
        }
    }
    normalized
}
