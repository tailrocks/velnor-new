//! Journal records that connect one authorized request to its capacity reservation.

use crate::error::HostError;

use super::read_table_columns;

const TABLE: &str = "connection_request_reservations";
const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS connection_request_reservations (connection_id TEXT NOT NULL, scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), request_id INTEGER NOT NULL CHECK (request_id > 0), reservation_id INTEGER NOT NULL UNIQUE REFERENCES capacity_reservations(id), physical_nonce TEXT NOT NULL CHECK (length(physical_nonce) = 32 AND physical_nonce NOT GLOB '*[^0-9a-f]*'), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), PRIMARY KEY (connection_id, scale_set_id, request_id), UNIQUE (connection_id, scale_set_id, physical_nonce), FOREIGN KEY (connection_id, scale_set_id, request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id))";
const RESERVATION_INDEX_DDL: &str = "CREATE INDEX IF NOT EXISTS connection_request_reservation_id ON connection_request_reservations(reservation_id)";
const INSERT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS connection_request_reservations_insert_guard BEFORE INSERT ON connection_request_reservations WHEN NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS auth JOIN capacity_reservations AS reservation ON reservation.id = NEW.reservation_id JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id AND provisioning.operation_id = intent.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id WHERE auth.connection_id = NEW.connection_id AND auth.scale_set_id = NEW.scale_set_id AND auth.request_id = NEW.request_id AND auth.acquire_state = 'requested' AND auth.started_event_digest IS NULL AND auth.completed_event_digest IS NULL AND auth.binding_digest = NEW.binding_digest AND reservation.state = 'reserved' AND reservation.connection_id = NEW.connection_id AND reservation.binding_digest = NEW.binding_digest AND reservation.assignment_key = 'v1|' || NEW.connection_id || '|' || NEW.scale_set_id || '|remote|6:runner|32:' || NEW.physical_nonce AND intent.connection_id = NEW.connection_id AND intent.scale_set_id = NEW.scale_set_id AND intent.kind = 'launch' AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND intent.assignment_key = reservation.assignment_key AND intent.runner_name = 'v' || NEW.physical_nonce AND intent.runner_request_id IS NULL AND intent.acquire_attempted = 0 AND intent.jit_requested = 0 AND intent.docker_id IS NULL AND intent.dind_id IS NULL AND intent.worker_volume IS NULL AND intent.launch_phase = 'provisioning_held' AND state.bootstrap_start_state = 'not_requested') BEGIN SELECT RAISE(ABORT, 'invalid physical request reservation link'); END";
const NO_UPDATE_TRIGGER: &str = "CREATE TRIGGER IF NOT EXISTS connection_request_reservations_no_update BEFORE UPDATE ON connection_request_reservations BEGIN SELECT RAISE(ABORT, 'immutable request reservation link'); END";
const NO_DELETE_TRIGGER: &str = "CREATE TRIGGER IF NOT EXISTS connection_request_reservations_no_delete BEFORE DELETE ON connection_request_reservations BEGIN SELECT RAISE(ABORT, 'immutable request reservation link'); END";

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        TABLE_DDL,
        RESERVATION_INDEX_DDL,
        INSERT_GUARD,
        NO_UPDATE_TRIGGER,
        NO_DELETE_TRIGGER,
    ] {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    validate_columns(connection).await?;
    validate_table(connection).await?;
    validate_index(connection).await?;
    validate_triggers(connection).await?;
    validate_rows(connection).await
}

async fn validate_columns(connection: &turso::Connection) -> Result<(), HostError> {
    let expected = [
        ("connection_id", "TEXT", true, 1),
        ("scale_set_id", "INTEGER", true, 2),
        ("request_id", "INTEGER", true, 3),
        ("reservation_id", "INTEGER", true, 0),
        ("physical_nonce", "TEXT", true, 0),
        ("binding_digest", "TEXT", true, 0),
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
    if canonical_sql(&actual) != canonical_sql(TABLE_DDL) {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_index(connection: &turso::Connection) -> Result<(), HostError> {
    let actual = object_sql(connection, "index", "connection_request_reservation_id").await?;
    if canonical_sql(&actual) != canonical_sql(RESERVATION_INDEX_DDL)
        || index_columns(connection).await? != ["reservation_id"]
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_triggers(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in [
        ("connection_request_reservations_insert_guard", INSERT_GUARD),
        (
            "connection_request_reservations_no_update",
            NO_UPDATE_TRIGGER,
        ),
        (
            "connection_request_reservations_no_delete",
            NO_DELETE_TRIGGER,
        ),
    ] {
        let actual = object_sql(connection, "trigger", name).await?;
        if canonical_sql(&actual) != canonical_sql(expected) {
            return Err(HostError::Journal);
        }
    }
    for (name, action) in [
        (
            "connection_request_reservations_revision_insert",
            "after insert",
        ),
        (
            "connection_request_reservations_revision_update",
            "after update",
        ),
        (
            "connection_request_reservations_revision_delete",
            "after delete",
        ),
    ] {
        let actual = canonical_sql(&object_sql(connection, "trigger", name).await?);
        let expected = canonical_sql(&format!(
            "CREATE TRIGGER {name} {action} ON {TABLE} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
        ));
        if actual != expected {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = scalar_count(
        connection,
        "SELECT COUNT(*) FROM connection_request_reservations AS link LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = link.connection_id AND auth.scale_set_id = link.scale_set_id AND auth.request_id = link.request_id LEFT JOIN capacity_reservations AS reservation ON reservation.id = link.reservation_id LEFT JOIN connection_intents AS intent ON intent.id = reservation.intent_id WHERE auth.request_id IS NULL OR auth.binding_digest != link.binding_digest OR auth.acquire_state = 'authorized' OR reservation.connection_id != link.connection_id OR reservation.binding_digest != link.binding_digest OR reservation.assignment_key != 'v1|' || link.connection_id || '|' || link.scale_set_id || '|remote|6:runner|32:' || link.physical_nonce OR intent.connection_id != link.connection_id OR intent.scale_set_id != link.scale_set_id OR intent.kind != 'launch' OR intent.assignment_key != reservation.assignment_key OR intent.runner_name != 'v' || link.physical_nonce OR intent.runner_request_id IS NOT NULL",
    )
    .await?;
    if invalid == 0 {
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
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn index_columns(connection: &turso::Connection) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query("PRAGMA index_info(connection_request_reservation_id)", ())
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

fn canonical_sql(sql: &str) -> String {
    let mut normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    for source in [
        "createtableifnotexists",
        "createindexifnotexists",
        "createuniqueindexifnotexists",
        "createtriggerifnotexists",
    ] {
        let replacement = source.trim_end_matches("ifnotexists");
        if normalized.starts_with(source) {
            normalized.replace_range(..source.len(), replacement);
            break;
        }
    }
    normalized
}
