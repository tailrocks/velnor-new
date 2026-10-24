//! Journal records for Doctor probe requests and their exact child probes.

use crate::error::HostError;

use super::read_table_columns;

const TABLE: &str = "doctor_probe_requests";
const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS doctor_probe_requests (request_id INTEGER PRIMARY KEY AUTOINCREMENT, nonce TEXT NOT NULL UNIQUE CHECK (length(nonce) = 32 AND nonce NOT GLOB '*[^0-9a-f]*'), instance_id TEXT NOT NULL CHECK (length(instance_id) = 32 AND instance_id NOT GLOB '*[^0-9a-f]*'), engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'recovering', 'uncertain', 'failed', 'completed', 'stale')), child_operation_id TEXT UNIQUE REFERENCES resource_probe_operations(operation_id), active_slot INTEGER NOT NULL DEFAULT 1 CHECK (active_slot = 1), CHECK ((state = 'queued' AND child_operation_id IS NULL) OR (state IN ('running', 'recovering', 'uncertain', 'completed', 'stale') AND child_operation_id IS NOT NULL) OR state = 'failed'))";
const ACTIVE_INDEX_DDL: &str = "CREATE UNIQUE INDEX IF NOT EXISTS doctor_probe_one_active ON doctor_probe_requests(active_slot) WHERE state IN ('queued', 'running', 'recovering', 'uncertain')";
const STATE_INDEX_DDL: &str =
    "CREATE INDEX IF NOT EXISTS doctor_probe_state ON doctor_probe_requests(state, request_id)";
const LINK_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS doctor_probe_link_guard BEFORE UPDATE OF child_operation_id ON doctor_probe_requests WHEN OLD.child_operation_id IS NOT NULL OR NEW.child_operation_id IS NULL OR OLD.state != 'queued' OR NEW.state != 'running' OR NOT EXISTS (SELECT 1 FROM resource_probe_operations AS p JOIN host_operation_leases AS l ON l.singleton = 1 AND l.operation_id = p.operation_id WHERE p.operation_id = NEW.child_operation_id AND p.instance_id = NEW.instance_id AND p.engine_id = NEW.engine_id AND p.docker_root_digest = NEW.docker_root_digest AND p.role = 'resource-probe' AND p.phase = 'prepared' AND p.container_id IS NULL AND l.state = 'reserved' AND l.engine_id = NEW.engine_id AND l.docker_root_digest = NEW.docker_root_digest) BEGIN SELECT RAISE(ABORT, 'invalid Doctor probe child'); END";
const TRANSITION_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS doctor_probe_transition_guard BEFORE UPDATE ON doctor_probe_requests WHEN OLD.request_id != NEW.request_id OR OLD.nonce != NEW.nonce OR OLD.instance_id != NEW.instance_id OR OLD.engine_id != NEW.engine_id OR OLD.docker_root_digest != NEW.docker_root_digest OR OLD.active_slot != NEW.active_slot OR (OLD.child_operation_id IS NOT NEW.child_operation_id AND NOT (OLD.state = 'queued' AND NEW.state = 'running' AND OLD.child_operation_id IS NULL AND NEW.child_operation_id IS NOT NULL)) OR (OLD.state != NEW.state AND NOT ((OLD.state = 'queued' AND NEW.state IN ('running', 'failed')) OR (OLD.state = 'running' AND NEW.state IN ('recovering', 'uncertain', 'completed', 'failed', 'stale')) OR (OLD.state = 'recovering' AND NEW.state IN ('uncertain', 'completed', 'failed', 'stale')) OR (OLD.state = 'uncertain' AND NEW.state = 'recovering') OR (OLD.state = 'completed' AND NEW.state = 'stale'))) BEGIN SELECT RAISE(ABORT, 'invalid Doctor probe transition'); END";
const TERMINAL_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS doctor_probe_terminal_guard BEFORE UPDATE OF state ON doctor_probe_requests WHEN NEW.state IN ('completed', 'failed', 'stale') AND NEW.child_operation_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM resource_probe_operations AS p WHERE p.operation_id = NEW.child_operation_id AND p.phase IN ('removed', 'aborted') AND NOT EXISTS (SELECT 1 FROM host_operation_leases AS l WHERE l.singleton = 1 AND l.operation_id = p.operation_id AND l.state != 'released') AND ((NEW.state = 'completed' AND p.phase = 'removed') OR NEW.state IN ('failed', 'stale'))) BEGIN SELECT RAISE(ABORT, 'Doctor probe still charged'); END";
const NO_DELETE_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS doctor_probe_no_delete BEFORE DELETE ON doctor_probe_requests BEGIN SELECT RAISE(ABORT, 'immutable Doctor probe ticket'); END";

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        TABLE_DDL,
        ACTIVE_INDEX_DDL,
        STATE_INDEX_DDL,
        LINK_GUARD,
        TRANSITION_GUARD,
        TERMINAL_GUARD,
        NO_DELETE_GUARD,
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
    validate_indexes(connection).await?;
    validate_guards(connection).await?;
    validate_rows(connection).await
}

async fn validate_columns(connection: &turso::Connection) -> Result<(), HostError> {
    let expected = [
        ("request_id", "INTEGER", false, 1),
        ("nonce", "TEXT", true, 0),
        ("instance_id", "TEXT", true, 0),
        ("engine_id", "TEXT", true, 0),
        ("docker_root_digest", "TEXT", true, 0),
        ("state", "TEXT", true, 0),
        ("child_operation_id", "TEXT", false, 0),
        ("active_slot", "INTEGER", true, 0),
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
            || column.default_value.is_some() != (name == "active_slot")
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

async fn validate_indexes(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA index_list(doctor_probe_requests)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut active = false;
    let mut state = false;
    let mut nonce_unique = false;
    let mut child_unique = false;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        let unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
        match name.as_str() {
            "doctor_probe_one_active" => {
                active = unique
                    && index_columns(connection, &name).await? == ["active_slot"]
                    && canonical_sql(&object_sql(connection, "index", &name).await?)
                        == canonical_sql(ACTIVE_INDEX_DDL);
            }
            "doctor_probe_state" => {
                state = !unique
                    && index_columns(connection, &name).await? == ["state", "request_id"]
                    && canonical_sql(&object_sql(connection, "index", &name).await?)
                        == canonical_sql(STATE_INDEX_DDL);
            }
            _ if unique && name.starts_with("sqlite_autoindex_doctor_probe_requests") => {
                let columns = index_columns(connection, &name).await?;
                nonce_unique |= columns == ["nonce"];
                child_unique |= columns == ["child_operation_id"];
            }
            _ => {}
        }
    }
    if active && state && nonce_unique && child_unique {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn validate_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, expected) in [
        ("doctor_probe_link_guard", LINK_GUARD),
        ("doctor_probe_transition_guard", TRANSITION_GUARD),
        ("doctor_probe_terminal_guard", TERMINAL_GUARD),
        ("doctor_probe_no_delete", NO_DELETE_GUARD),
    ] {
        if canonical_sql(&object_sql(connection, "trigger", name).await?) != canonical_sql(expected)
        {
            return Err(HostError::Journal);
        }
    }
    for (name, table, action) in [
        (
            "doctor_probe_requests_revision_insert",
            TABLE,
            "after insert",
        ),
        (
            "doctor_probe_requests_revision_update",
            TABLE,
            "after update",
        ),
        (
            "doctor_probe_requests_revision_delete",
            TABLE,
            "after delete",
        ),
    ] {
        let actual = canonical_sql(&object_sql(connection, "trigger", name).await?);
        let expected = canonical_sql(&format!(
            "CREATE TRIGGER {name} {action} ON {table} BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END"
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
        "SELECT COUNT(*) FROM doctor_probe_requests AS request JOIN journal_meta AS meta ON meta.singleton = 1 LEFT JOIN resource_probe_operations AS probe ON probe.operation_id = request.child_operation_id LEFT JOIN host_operation_leases AS lease ON lease.singleton = 1 WHERE request.instance_id != meta.instance_id OR meta.engine_id IS NULL OR request.engine_id != meta.engine_id OR request.active_slot != 1 OR (request.state = 'queued' AND request.child_operation_id IS NOT NULL) OR (request.state IN ('running', 'recovering', 'uncertain', 'completed', 'stale') AND (request.child_operation_id IS NULL OR probe.operation_id IS NULL OR probe.instance_id != request.instance_id OR probe.engine_id != request.engine_id OR probe.docker_root_digest != request.docker_root_digest OR probe.role != 'resource-probe')) OR (request.state IN ('completed', 'stale') AND (probe.phase NOT IN ('removed', 'aborted') OR (lease.operation_id = request.child_operation_id AND lease.state != 'released') OR (request.state = 'completed' AND probe.phase != 'removed'))) OR (request.state = 'failed' AND request.child_operation_id IS NOT NULL AND (probe.phase NOT IN ('removed', 'aborted') OR (lease.operation_id = request.child_operation_id AND lease.state != 'released'))) OR (request.state = 'uncertain' AND (probe.phase != 'quarantined' OR lease.operation_id IS NOT request.child_operation_id OR lease.state != 'uncertain')) OR (request.state = 'running' AND (probe.phase IN ('removed', 'aborted', 'quarantined') OR lease.operation_id IS NOT request.child_operation_id OR lease.state = 'released')) OR (request.state = 'recovering' AND (probe.phase IN ('removed', 'aborted') OR lease.operation_id IS NOT request.child_operation_id OR lease.state = 'released'))",
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

async fn scalar_count(connection: &turso::Connection, query: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(query, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    row.get(0).map_err(|_| HostError::Journal)
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
