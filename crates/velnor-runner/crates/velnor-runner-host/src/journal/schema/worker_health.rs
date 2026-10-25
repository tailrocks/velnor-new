use crate::error::HostError;

const TABLE: &str = concat!(
    "CREATE TABLE local_worker_exits (",
    "lineage_id INTEGER NOT NULL CHECK (lineage_id > 0), ",
    "generation INTEGER NOT NULL CHECK (generation > 0), ",
    "reservation_id INTEGER NOT NULL, ",
    "intent_id INTEGER NOT NULL CHECK (intent_id > 0), ",
    "connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), ",
    "scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), ",
    "binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), ",
    "engine_id TEXT NOT NULL CHECK (length(CAST(engine_id AS BLOB)) BETWEEN 1 AND 256), ",
    "docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), ",
    "runner_container_id TEXT NOT NULL CHECK (length(CAST(runner_container_id AS BLOB)) BETWEEN 1 AND 256), ",
    "kind TEXT NOT NULL CHECK (kind IN ('exited', 'missing')), ",
    "exit_code INTEGER, ",
    "observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms >= 0), ",
    "CHECK ((kind = 'exited' AND exit_code IS NOT NULL AND exit_code BETWEEN -2147483648 AND 2147483647) OR (kind = 'missing' AND exit_code IS NULL)), ",
    "PRIMARY KEY (lineage_id, generation), ",
    "FOREIGN KEY (lineage_id, generation, reservation_id) REFERENCES physical_attempts(lineage_id, generation, reservation_id)",
    ")"
);

const INDEX: &str = "CREATE INDEX local_worker_exits_connection_observed ON local_worker_exits(connection_id, observed_at_unix_ms, lineage_id, generation)";

const INSERT_GUARD: &str = concat!(
    "CREATE TRIGGER local_worker_exit_insert_guard BEFORE INSERT ON local_worker_exits ",
    "WHEN NOT EXISTS (SELECT 1 FROM physical_attempt_current AS \"current\" ",
    "JOIN physical_attempts AS attempt ON attempt.lineage_id = \"current\".lineage_id AND attempt.generation = \"current\".generation AND attempt.reservation_id = \"current\".reservation_id AND attempt.intent_id = \"current\".intent_id ",
    "JOIN capacity_reservations AS reservation ON reservation.id = \"current\".reservation_id AND reservation.intent_id = \"current\".intent_id AND reservation.connection_id = attempt.connection_id ",
    "JOIN connection_intents AS intent ON intent.id = \"current\".intent_id AND intent.connection_id = attempt.connection_id ",
    "JOIN connection_runtime_bindings AS runtime ON runtime.connection_id = attempt.connection_id AND runtime.scale_set_id = attempt.scale_set_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id AND jit.scale_set_id = attempt.scale_set_id ",
    "JOIN connection_request_authorizations AS witness ON witness.connection_id = attempt.connection_id AND witness.scale_set_id = attempt.scale_set_id AND witness.request_id = attempt.witness_request_id ",
    "WHERE \"current\".lineage_id = NEW.lineage_id AND \"current\".generation = NEW.generation AND \"current\".reservation_id = NEW.reservation_id AND \"current\".intent_id = NEW.intent_id ",
    "AND attempt.connection_id = NEW.connection_id AND attempt.scale_set_id = NEW.scale_set_id AND attempt.binding_digest = NEW.binding_digest ",
    "AND attempt.engine_id = NEW.engine_id AND attempt.docker_root_digest = NEW.docker_root_digest ",
    "AND reservation.state IN ('reserved', 'uncertain', 'recovering') AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND reservation.assignment_key = intent.assignment_key ",
    "AND intent.kind = 'launch' AND intent.scale_set_id = attempt.scale_set_id AND intent.state IN ('pending', 'uncertain', 'done') AND intent.cleanup_proven = 0 AND intent.worker_cleanup_proven = 0 AND intent.runner_completed = 0 AND (intent.runner_request_id IS NULL OR intent.runner_request_id = jit.started_request_id) AND intent.docker_id = NEW.runner_container_id ",
    "AND ((jit.state = 'ready' AND intent.launch_phase = 'worker_ready') OR (jit.state = 'started' AND intent.launch_phase = 'runner_started')) ",
    "AND jit.completed_request_id IS NULL AND witness.acquire_state = 'acquired' AND witness.authority_model = 'created_empty_exclusive_admin_session_v1' AND witness.binding_digest = attempt.binding_digest AND witness.completed_event_digest IS NULL ",
    "AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS completed WHERE completed.connection_id = attempt.connection_id AND completed.scale_set_id = attempt.scale_set_id AND completed.completed_intent_id = attempt.intent_id AND completed.completed_event_digest IS NOT NULL)) ",
    "BEGIN SELECT RAISE(ABORT, 'invalid local worker exit'); END"
);

const NO_UPDATE: &str = "CREATE TRIGGER local_worker_exit_no_update BEFORE UPDATE ON local_worker_exits BEGIN SELECT RAISE(ABORT, 'local worker exit is immutable'); END";
const NO_DELETE: &str = "CREATE TRIGGER local_worker_exit_no_delete BEFORE DELETE ON local_worker_exits BEGIN SELECT RAISE(ABORT, 'local worker exit is immutable'); END";
const REVISION_INSERT: &str = "CREATE TRIGGER local_worker_exits_revision_insert AFTER INSERT ON local_worker_exits BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END";
const REVISION_UPDATE: &str = "CREATE TRIGGER local_worker_exits_revision_update AFTER UPDATE ON local_worker_exits BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END";
const REVISION_DELETE: &str = "CREATE TRIGGER local_worker_exits_revision_delete AFTER DELETE ON local_worker_exits BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END";

const OBJECTS: [(&str, &str, &str); 8] = [
    ("table", "local_worker_exits", TABLE),
    ("index", "local_worker_exits_connection_observed", INDEX),
    ("trigger", "local_worker_exit_insert_guard", INSERT_GUARD),
    ("trigger", "local_worker_exit_no_update", NO_UPDATE),
    ("trigger", "local_worker_exit_no_delete", NO_DELETE),
    (
        "trigger",
        "local_worker_exits_revision_insert",
        REVISION_INSERT,
    ),
    (
        "trigger",
        "local_worker_exits_revision_update",
        REVISION_UPDATE,
    ),
    (
        "trigger",
        "local_worker_exits_revision_delete",
        REVISION_DELETE,
    ),
];

#[cfg(test)]
#[path = "worker_health/tests.rs"]
mod tests;

pub(super) async fn migrate_v40_to_v41(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v40(conn).await?;
    for (_, _, sql) in OBJECTS {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    conn.execute("PRAGMA user_version = 41", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v41(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, expected) in OBJECTS {
        validate_object(conn, kind, name, expected).await?;
    }
    validate_rows(conn).await
}

async fn validate_object(
    conn: &turso::Connection,
    kind: &str,
    name: &str,
    expected: &str,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
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
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || super::sql_normalization::normalize(&actual)
            != super::sql_normalization::normalize(expected)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM local_worker_exits AS exit LEFT JOIN physical_attempts AS attempt ON attempt.lineage_id = exit.lineage_id AND attempt.generation = exit.generation AND attempt.reservation_id = exit.reservation_id AND attempt.intent_id = exit.intent_id LEFT JOIN connection_intents AS intent ON intent.id = exit.intent_id AND intent.connection_id = exit.connection_id WHERE attempt.lineage_id IS NULL OR intent.id IS NULL OR attempt.connection_id != exit.connection_id OR attempt.scale_set_id != exit.scale_set_id OR attempt.binding_digest != exit.binding_digest OR attempt.engine_id != exit.engine_id OR attempt.docker_root_digest != exit.docker_root_digest OR intent.docker_id != exit.runner_container_id",
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
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}
