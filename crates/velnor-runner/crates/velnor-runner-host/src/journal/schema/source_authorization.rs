//! Durable authenticated source receipts for assigned requests.

use crate::error::HostError;
use sha2::{Digest, Sha256};

use super::read_table_columns;

const TABLE: &str = "connection_source_authorizations";

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS connection_source_authorizations (connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), request_id INTEGER NOT NULL CHECK (request_id > 0), intent_id INTEGER NOT NULL UNIQUE REFERENCES connection_intents(id), message_id INTEGER NOT NULL CHECK (message_id >= 0), policy_mode TEXT NOT NULL CHECK (policy_mode IN ('repository', 'branch_restricted')), authority_model TEXT NOT NULL CHECK (authority_model = 'created_empty_exclusive_admin_session_v1'), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'), source_json TEXT NOT NULL CHECK (length(CAST(source_json AS BLOB)) BETWEEN 1 AND 32768), PRIMARY KEY (connection_id, scale_set_id, request_id))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS connection_source_authorization_intent ON connection_source_authorizations(connection_id, scale_set_id, intent_id)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    validate_v8(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validate_columns(conn, true).await?;
    validate_table_sql(conn, true).await?;
    validate_indexes(conn).await?;
    validate_rows(conn, true).await
}

pub(super) async fn upgrade_v8(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v8(conn).await?;
    conn.execute(
        "ALTER TABLE connection_source_authorizations ADD COLUMN receipt_binding_digest TEXT CHECK (receipt_binding_digest IS NULL OR (length(receipt_binding_digest) = 64 AND receipt_binding_digest NOT GLOB '*[^0-9a-f]*'))",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn validate_v8(conn: &turso::Connection) -> Result<(), HostError> {
    validate_columns(conn, false).await?;
    validate_table_sql(conn, false).await?;
    validate_indexes(conn).await?;
    validate_rows(conn, false).await?;
    Ok(())
}

async fn validate_columns(
    conn: &turso::Connection,
    include_receipt_binding: bool,
) -> Result<(), HostError> {
    let columns = read_table_columns(conn, TABLE).await?;
    let expected = [
        ("connection_id", "TEXT", true, 1),
        ("scale_set_id", "INTEGER", true, 2),
        ("request_id", "INTEGER", true, 3),
        ("intent_id", "INTEGER", true, 0),
        ("message_id", "INTEGER", true, 0),
        ("policy_mode", "TEXT", true, 0),
        ("authority_model", "TEXT", true, 0),
        ("binding_digest", "TEXT", true, 0),
        ("source_digest", "TEXT", true, 0),
        ("source_json", "TEXT", true, 0),
    ];
    let optional_columns = usize::from(include_receipt_binding);
    if columns.len() != expected.len() + optional_columns {
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
    if include_receipt_binding {
        let Some(column) = columns.get("receipt_binding_digest") else {
            return Err(HostError::Journal);
        };
        if !column.declared_type.eq_ignore_ascii_case("TEXT")
            || column.not_null
            || column.primary_key_position != 0
            || column.default_value.is_some()
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_table_sql(
    conn: &turso::Connection,
    include_receipt_binding: bool,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [TABLE],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?
        .to_ascii_lowercase()
        .replace(char::is_whitespace, "");
    for required in [
        "referencesconnection_scopes(connection_id)",
        "referencesconnection_intents(id)",
        "check(scale_set_id>0)",
        "check(request_id>0)",
        "check(message_id>=0)",
        "check(policy_modein('repository','branch_restricted'))",
        "check(authority_model='created_empty_exclusive_admin_session_v1')",
        "check(length(binding_digest)=64andbinding_digestnotglob'*[^0-9a-f]*')",
        "check(length(source_digest)=64andsource_digestnotglob'*[^0-9a-f]*')",
        "check(length(cast(source_jsonasblob))between1and32768)",
        "primarykey(connection_id,scale_set_id,request_id)",
    ] {
        if !sql.contains(required) {
            return Err(HostError::Journal);
        }
    }
    if include_receipt_binding
        && !sql.contains(
            "check(receipt_binding_digestisnullor(length(receipt_binding_digest)=64andreceipt_binding_digestnotglob'*[^0-9a-f]*'))",
        )
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_indexes(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query("PRAGMA index_list(connection_source_authorizations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut unique_intent = false;
    let mut intent_index = false;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
        if unique && name.starts_with("sqlite_autoindex_connection_source_authorizations") {
            unique_intent = index_columns(conn, &name).await? == ["intent_id"];
        }
        if name == "connection_source_authorization_intent" {
            intent_index =
                index_columns(conn, &name).await? == ["connection_id", "scale_set_id", "intent_id"];
        }
    }
    if unique_intent && intent_index {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn index_columns(conn: &turso::Connection, index: &str) -> Result<Vec<String>, HostError> {
    let mut rows = conn
        .query(&format!("PRAGMA index_info({index})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(row.get(2).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}

async fn validate_rows(
    conn: &turso::Connection,
    include_receipt_binding: bool,
) -> Result<(), HostError> {
    let receipt_binding = if include_receipt_binding {
        "c.receipt_binding_digest"
    } else {
        "NULL"
    };
    let mut rows = conn
        .query(
            &format!("SELECT c.connection_id, c.scale_set_id, c.request_id, c.intent_id, c.message_id, c.policy_mode, c.authority_model, c.binding_digest, {receipt_binding}, c.source_digest, c.source_json, (SELECT COUNT(*) FROM connection_intents AS i WHERE i.id = c.intent_id AND i.connection_id = c.connection_id AND i.scale_set_id = c.scale_set_id AND i.runner_request_id = c.request_id AND i.kind = 'launch'), (SELECT identity FROM connection_scopes AS s WHERE s.connection_id = c.connection_id) FROM connection_source_authorizations AS c"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let connection_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let scale_set_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let request_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let intent_id: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        let message_id: i64 = row.get(4).map_err(|_| HostError::Journal)?;
        let policy_mode: String = row.get(5).map_err(|_| HostError::Journal)?;
        let authority_model: String = row.get(6).map_err(|_| HostError::Journal)?;
        let binding_digest: String = row.get(7).map_err(|_| HostError::Journal)?;
        let receipt_binding_digest: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
        let source_digest: String = row.get(9).map_err(|_| HostError::Journal)?;
        let source_json: String = row.get(10).map_err(|_| HostError::Journal)?;
        let matching_intent: i64 = row.get(11).map_err(|_| HostError::Journal)?;
        let identity: String = row.get(12).map_err(|_| HostError::Journal)?;
        let identity_value: serde_json::Value =
            serde_json::from_str(&identity).map_err(|_| HostError::Journal)?;
        let repository_id = identity_value
            .get("repository_id")
            .and_then(serde_json::Value::as_i64)
            .filter(|value| *value > 0)
            .ok_or(HostError::Journal)?;
        if connection_id.is_empty()
            || scale_set_id <= 0
            || request_id <= 0
            || intent_id <= 0
            || message_id < 0
            || !matches!(policy_mode.as_str(), "repository" | "branch_restricted")
            || authority_model != "created_empty_exclusive_admin_session_v1"
            || !valid_digest(&binding_digest)
            || digest(identity.as_bytes()) != binding_digest
            || receipt_binding_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value) || value != binding_digest)
            || !valid_digest(&source_digest)
            || source_json.is_empty()
            || source_json.len() > 32768
            || digest(source_json.as_bytes()) != source_digest
            || matching_intent != 1
        {
            return Err(HostError::Journal);
        }
        crate::journal::source_authorization::validate_stored_source(&source_json, repository_id)?;
    }
    Ok(())
}

fn digest(value: &[u8]) -> String {
    let bytes = Sha256::digest(value);
    let mut result = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _written = write!(result, "{byte:02x}");
    }
    result
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
