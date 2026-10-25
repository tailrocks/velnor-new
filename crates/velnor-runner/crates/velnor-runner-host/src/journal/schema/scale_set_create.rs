//! Durable Scale Set creation schema.

use crate::error::HostError;

use super::read_table_columns;

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS connection_scale_set_intents (id INTEGER PRIMARY KEY AUTOINCREMENT, connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), repository_id INTEGER NOT NULL CHECK (repository_id > 0), repository TEXT NOT NULL CHECK (length(repository) BETWEEN 3 AND 201), selector_name TEXT NOT NULL CHECK (length(selector_name) BETWEEN 1 AND 64), selector_labels TEXT NOT NULL CHECK (length(CAST(selector_labels AS BLOB)) BETWEEN 1 AND 4096), runner_group_id INTEGER NOT NULL CHECK (runner_group_id > 0), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'no_effect', 'created', 'adopted')), no_effect_status INTEGER, remote_id INTEGER, remote_name TEXT, remote_labels TEXT, disable_update INTEGER, ownership TEXT CHECK (ownership IS NULL OR ownership IN ('created', 'adopted')), CHECK ((state = 'no_effect' AND no_effect_status = 409 AND remote_id IS NULL AND remote_name IS NULL AND remote_labels IS NULL AND disable_update IS NULL AND ownership IS NULL) OR (state IN ('prepared', 'requested', 'uncertain') AND no_effect_status IS NULL AND remote_id IS NULL AND remote_name IS NULL AND remote_labels IS NULL AND disable_update IS NULL AND ownership IS NULL) OR (state IN ('created', 'adopted') AND (no_effect_status IS NULL OR (state = 'adopted' AND no_effect_status = 409)) AND remote_id > 0 AND remote_name = selector_name AND remote_labels IS NOT NULL AND disable_update = 1 AND ownership = state)), UNIQUE (connection_id))",
        "CREATE INDEX IF NOT EXISTS connection_scale_set_state ON connection_scale_set_intents(state, connection_id)",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    let expected = [
        ("id", "INTEGER", false, 1),
        ("connection_id", "TEXT", true, 0),
        ("binding_digest", "TEXT", true, 0),
        ("repository_id", "INTEGER", true, 0),
        ("repository", "TEXT", true, 0),
        ("selector_name", "TEXT", true, 0),
        ("selector_labels", "TEXT", true, 0),
        ("runner_group_id", "INTEGER", true, 0),
        ("state", "TEXT", true, 0),
        ("no_effect_status", "INTEGER", false, 0),
        ("remote_id", "INTEGER", false, 0),
        ("remote_name", "TEXT", false, 0),
        ("remote_labels", "TEXT", false, 0),
        ("disable_update", "INTEGER", false, 0),
        ("ownership", "TEXT", false, 0),
    ];
    let columns = read_table_columns(conn, "connection_scale_set_intents").await?;
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
    validate_table_sql(conn).await?;
    validate_indexes(conn).await
}

async fn validate_table_sql(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'connection_scale_set_intents'",
            (),
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
    for fragment in [
        "idintegerprimarykeyautoincrement",
        "connection_idtextnotnullreferencesconnection_scopes(connection_id)",
        "check(length(binding_digest)=64andbinding_digestnotglob'*[^0-9a-f]*')",
        "check(repository_id>0)",
        "check(length(repository)between3and201)",
        "check(length(selector_name)between1and64)",
        "check(length(cast(selector_labelsasblob))between1and4096)",
        "check(runner_group_id>0)",
        "check(statein('prepared','requested','uncertain','no_effect','created','adopted'))",
        "check(ownershipisnullorownershipin('created','adopted'))",
        "check((state='no_effect'andno_effect_status=409andremote_idisnullandremote_nameisnullandremote_labelsisnullanddisable_updateisnullandownershipisnull)or(statein('prepared','requested','uncertain')andno_effect_statusisnullandremote_idisnullandremote_nameisnullandremote_labelsisnullanddisable_updateisnullandownershipisnull)or(statein('created','adopted')and(no_effect_statusisnullor(state='adopted'andno_effect_status=409))andremote_id>0andremote_name=selector_nameandremote_labelsisnotnullanddisable_update=1andownership=state))",
        "unique(connection_id)",
    ] {
        if !sql.contains(fragment) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_indexes(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query("PRAGMA index_list(connection_scale_set_intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut unique_connection = false;
    let mut state_index = false;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
        if unique && name.starts_with("sqlite_autoindex_connection_scale_set_intents") {
            unique_connection = index_columns(conn, &name).await? == ["connection_id"];
        }
        if name == "connection_scale_set_state" {
            state_index = index_columns(conn, &name).await? == ["state", "connection_id"];
        }
    }
    if unique_connection && state_index {
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
