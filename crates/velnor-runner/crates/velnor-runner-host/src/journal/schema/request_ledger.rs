//! Durable request receipts and physical runner bindings.

mod validation;

use crate::error::HostError;

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS connection_request_authorizations (connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), request_id INTEGER NOT NULL CHECK (request_id > 0), message_id INTEGER NOT NULL CHECK (message_id >= 0), policy_mode TEXT NOT NULL CHECK (policy_mode IN ('repository', 'branch_restricted')), authority_model TEXT NOT NULL CHECK (authority_model = 'created_empty_exclusive_admin_session_v1'), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'), source_json TEXT NOT NULL CHECK (length(CAST(source_json AS BLOB)) BETWEEN 1 AND 32768), acquire_state TEXT NOT NULL CHECK (acquire_state IN ('authorized', 'requested', 'acquired', 'no_effect', 'already_acquired', 'rejected', 'uncertain')), acquire_response_json TEXT CHECK (acquire_response_json IS NULL OR length(CAST(acquire_response_json AS BLOB)) <= 4096), acquire_response_digest TEXT CHECK (acquire_response_digest IS NULL OR (length(acquire_response_digest) = 64 AND acquire_response_digest NOT GLOB '*[^0-9a-f]*')), started_message_id INTEGER CHECK (started_message_id IS NULL OR started_message_id >= 0), started_event_digest TEXT CHECK (started_event_digest IS NULL OR (length(started_event_digest) = 64 AND started_event_digest NOT GLOB '*[^0-9a-f]*')), started_job_id TEXT CHECK (started_job_id IS NULL OR length(CAST(started_job_id AS BLOB)) <= 256), started_runner_id INTEGER CHECK (started_runner_id IS NULL OR started_runner_id > 0), started_runner_name TEXT CHECK (started_runner_name IS NULL OR length(CAST(started_runner_name AS BLOB)) BETWEEN 1 AND 128), completed_message_id INTEGER CHECK (completed_message_id IS NULL OR completed_message_id >= 0), completed_event_digest TEXT CHECK (completed_event_digest IS NULL OR (length(completed_event_digest) = 64 AND completed_event_digest NOT GLOB '*[^0-9a-f]*')), completed_job_id TEXT CHECK (completed_job_id IS NULL OR length(CAST(completed_job_id AS BLOB)) <= 256), completed_result TEXT CHECK (completed_result IS NULL OR length(CAST(completed_result AS BLOB)) <= 128), completed_runner_id INTEGER CHECK (completed_runner_id IS NULL OR completed_runner_id > 0), completed_runner_name TEXT CHECK (completed_runner_name IS NULL OR length(CAST(completed_runner_name AS BLOB)) BETWEEN 1 AND 128), completed_intent_id INTEGER REFERENCES connection_intents(id), PRIMARY KEY (connection_id, scale_set_id, request_id), CHECK ((acquire_state IN ('authorized', 'requested') AND acquire_response_json IS NULL AND acquire_response_digest IS NULL) OR (acquire_state NOT IN ('authorized', 'requested') AND acquire_response_json IS NOT NULL AND acquire_response_digest IS NOT NULL)), CHECK ((started_event_digest IS NULL AND started_message_id IS NULL AND started_job_id IS NULL AND started_runner_id IS NULL AND started_runner_name IS NULL) OR (started_event_digest IS NOT NULL AND started_message_id IS NOT NULL AND started_runner_id IS NOT NULL AND started_runner_name IS NOT NULL)), CHECK ((completed_event_digest IS NULL AND completed_message_id IS NULL AND completed_job_id IS NULL AND completed_result IS NULL AND completed_runner_id IS NULL AND completed_runner_name IS NULL AND completed_intent_id IS NULL) OR (completed_event_digest IS NOT NULL AND completed_message_id IS NOT NULL AND ((completed_runner_id IS NULL AND completed_runner_name IS NULL AND completed_intent_id IS NULL) OR (completed_runner_id IS NOT NULL AND completed_runner_name IS NOT NULL AND completed_intent_id IS NOT NULL)))))",
        "CREATE INDEX IF NOT EXISTS connection_request_authorization_state ON connection_request_authorizations(connection_id, scale_set_id, acquire_state, request_id)",
        "CREATE INDEX IF NOT EXISTS connection_request_completion_intent ON connection_request_authorizations(connection_id, scale_set_id, completed_intent_id) WHERE completed_intent_id IS NOT NULL",
        "CREATE TABLE IF NOT EXISTS connection_runner_jits (intent_id INTEGER PRIMARY KEY REFERENCES connection_intents(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), witness_request_id INTEGER NOT NULL CHECK (witness_request_id > 0), requested_name TEXT NOT NULL CHECK (length(CAST(requested_name AS BLOB)) BETWEEN 1 AND 128), state TEXT NOT NULL CHECK (state IN ('claimed', 'uncertain', 'bound', 'start_requested', 'start_uncertain', 'ready', 'started', 'completed', 'cleaned')), runner_id INTEGER CHECK (runner_id IS NULL OR runner_id > 0), runner_name TEXT CHECK (runner_name IS NULL OR length(CAST(runner_name AS BLOB)) BETWEEN 1 AND 128), started_request_id INTEGER CHECK (started_request_id IS NULL OR started_request_id > 0), completed_request_id INTEGER CHECK (completed_request_id IS NULL OR completed_request_id > 0), FOREIGN KEY (connection_id, scale_set_id, witness_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id), FOREIGN KEY (connection_id, scale_set_id, started_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id), FOREIGN KEY (connection_id, scale_set_id, completed_request_id) REFERENCES connection_request_authorizations(connection_id, scale_set_id, request_id), CHECK ((state IN ('claimed', 'uncertain') AND runner_id IS NULL AND runner_name IS NULL) OR (state NOT IN ('claimed', 'uncertain') AND runner_id IS NOT NULL AND runner_name IS NOT NULL)), CHECK (state != 'started' OR started_request_id IS NOT NULL), CHECK (state != 'completed' OR completed_request_id IS NOT NULL))",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_runner_jit_id ON connection_runner_jits(connection_id, scale_set_id, runner_id) WHERE runner_id IS NOT NULL",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_runner_jit_name ON connection_runner_jits(connection_id, scale_set_id, runner_name) WHERE runner_name IS NOT NULL AND state != 'cleaned'",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state != 'cleaned'",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_runner_jit_started_request ON connection_runner_jits(connection_id, scale_set_id, started_request_id) WHERE started_request_id IS NOT NULL",
        "CREATE UNIQUE INDEX IF NOT EXISTS connection_runner_jit_completed_request ON connection_runner_jits(connection_id, scale_set_id, completed_request_id) WHERE completed_request_id IS NOT NULL",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validation::validate(conn).await
}

pub(super) async fn upgrade_cleaned_runner_state(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validation::validate_v10(conn).await?;
    let old_sql = jit_table_sql(conn).await?;
    let old_table = "CREATE TABLE connection_runner_jits (";
    let old_check =
        "CHECK (state NOT IN ('completed', 'cleaned') OR completed_request_id IS NOT NULL)";
    if old_sql.matches(old_table).count() != 1 || old_sql.matches(old_check).count() != 1 {
        return Err(HostError::Journal);
    }
    let new_sql = old_sql
        .replacen(old_table, "CREATE TABLE connection_runner_jits_v11 (", 1)
        .replacen(
            old_check,
            "CHECK (state != 'completed' OR completed_request_id IS NOT NULL)",
            1,
        );
    conn.execute(&new_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(
        "INSERT INTO connection_runner_jits_v11 (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name, started_request_id, completed_request_id) SELECT j.intent_id, j.connection_id, j.scale_set_id, j.witness_request_id, j.requested_name, CASE WHEN j.state = 'ready' AND j.runner_id IS NOT NULL AND j.runner_name IS NOT NULL AND EXISTS (SELECT 1 FROM connection_intents AS i WHERE i.id = j.intent_id AND i.connection_id = j.connection_id AND i.cleanup_proven = 1) AND EXISTS (SELECT 1 FROM capacity_reservations AS r WHERE r.intent_id = j.intent_id AND r.connection_id = j.connection_id AND r.state = 'released' AND r.remote_cleanup_proven = 1 AND r.local_cleanup_proven = 1) THEN 'cleaned' ELSE j.state END, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id FROM connection_runner_jits AS j",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    conn.execute("DROP TABLE connection_runner_jits", ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(
        "ALTER TABLE connection_runner_jits_v11 RENAME TO connection_runner_jits",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    for index in [
        "CREATE UNIQUE INDEX connection_runner_jit_id ON connection_runner_jits(connection_id, scale_set_id, runner_id) WHERE runner_id IS NOT NULL",
        "CREATE UNIQUE INDEX connection_runner_jit_name ON connection_runner_jits(connection_id, scale_set_id, runner_name) WHERE runner_name IS NOT NULL AND state != 'cleaned'",
        "CREATE UNIQUE INDEX connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state != 'cleaned'",
        "CREATE UNIQUE INDEX connection_runner_jit_started_request ON connection_runner_jits(connection_id, scale_set_id, started_request_id) WHERE started_request_id IS NOT NULL",
        "CREATE UNIQUE INDEX connection_runner_jit_completed_request ON connection_runner_jits(connection_id, scale_set_id, completed_request_id) WHERE completed_request_id IS NOT NULL",
    ] {
        conn.execute(index, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validation::validate(conn).await
}

async fn jit_table_sql(conn: &turso::Connection) -> Result<String, HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'connection_runner_jits'",
            (),
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

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate(conn).await
}
