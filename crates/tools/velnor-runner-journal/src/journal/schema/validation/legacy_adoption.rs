//! Validation for immutable Option A adoption provenance.

use crate::error::HostError;

pub(in crate::journal::schema) const TABLE_SQL: &str = "CREATE TABLE linux_launch_daemon_adoptions (launch_id INTEGER PRIMARY KEY NOT NULL CHECK (launch_id > 0), endpoint TEXT NOT NULL CHECK (length(endpoint) BETWEEN 2 AND 4096), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 1024), evidence_kind TEXT NOT NULL CHECK (evidence_kind = 'started_actions_completed_inventory_v1'), github_runner_id TEXT NOT NULL, runner_name TEXT NOT NULL, workflow_run_id INTEGER NOT NULL CHECK (workflow_run_id > 0), scale_set_job_id TEXT NOT NULL, actions_attempt INTEGER NOT NULL CHECK (actions_attempt BETWEEN 1 AND 8), actions_job_id INTEGER NOT NULL CHECK (actions_job_id > 0), actions_conclusion TEXT, docker_id TEXT NOT NULL, dind_id TEXT NOT NULL, worker_volume TEXT NOT NULL, outer_network_name TEXT NOT NULL, outer_network_id TEXT NOT NULL, adopted_at_ms INTEGER NOT NULL CHECK (adopted_at_ms >= 0), FOREIGN KEY (launch_id) REFERENCES intents(id))";
pub(in crate::journal::schema) const STARTED_TABLE_SQL: &str = "CREATE TABLE linux_launch_started_observations (launch_id INTEGER PRIMARY KEY NOT NULL CHECK (launch_id > 0), observed_at_ms INTEGER NOT NULL CHECK (observed_at_ms >= 0), FOREIGN KEY (launch_id) REFERENCES intents(id))";

pub(super) async fn validate_started_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_started_columns(conn).await?;
    let mut rows = conn
        .query("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'linux_launch_started_observations'", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if normalize_sql(&actual) != normalize_sql(STARTED_TABLE_SQL)
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM linux_launch_started_observations AS s LEFT JOIN intents AS i ON i.id = s.launch_id WHERE i.id IS NULL OR i.kind != 'launch' OR i.github_runner_id IS NULL OR i.runner_name IS NULL OR i.observed_job_id IS NULL OR i.observed_workflow_run_id IS NULL",
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

async fn validate_started_columns(conn: &turso::Connection) -> Result<(), HostError> {
    const COLUMNS: [(&str, &str, i64, i64); 2] = [
        ("launch_id", "INTEGER", 1, 1),
        ("observed_at_ms", "INTEGER", 1, 0),
    ];
    let mut rows = conn
        .query("PRAGMA table_info(linux_launch_started_observations)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (name, kind, not_null, primary_key) in COLUMNS {
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        if row.get::<String>(1).map_err(|_| HostError::Journal)? != name
            || row.get::<String>(2).map_err(|_| HostError::Journal)? != kind
            || row.get::<i64>(3).map_err(|_| HostError::Journal)? != not_null
            || row.get::<i64>(5).map_err(|_| HostError::Journal)? != primary_key
        {
            return Err(HostError::Journal);
        }
    }
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn validate_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_columns(conn).await?;
    validate_definition(conn).await?;
    validate_rows(conn).await
}

async fn validate_columns(conn: &turso::Connection) -> Result<(), HostError> {
    const COLUMNS: [(&str, &str, i64, i64); 17] = [
        ("launch_id", "INTEGER", 1, 1),
        ("endpoint", "TEXT", 1, 0),
        ("engine_id", "TEXT", 1, 0),
        ("evidence_kind", "TEXT", 1, 0),
        ("github_runner_id", "TEXT", 1, 0),
        ("runner_name", "TEXT", 1, 0),
        ("workflow_run_id", "INTEGER", 1, 0),
        ("scale_set_job_id", "TEXT", 1, 0),
        ("actions_attempt", "INTEGER", 1, 0),
        ("actions_job_id", "INTEGER", 1, 0),
        ("actions_conclusion", "TEXT", 0, 0),
        ("docker_id", "TEXT", 1, 0),
        ("dind_id", "TEXT", 1, 0),
        ("worker_volume", "TEXT", 1, 0),
        ("outer_network_name", "TEXT", 1, 0),
        ("outer_network_id", "TEXT", 1, 0),
        ("adopted_at_ms", "INTEGER", 1, 0),
    ];
    let mut rows = conn
        .query("PRAGMA table_info(linux_launch_daemon_adoptions)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (name, kind, not_null, primary_key) in COLUMNS {
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        if row.get::<String>(1).map_err(|_| HostError::Journal)? != name
            || row.get::<String>(2).map_err(|_| HostError::Journal)? != kind
            || row.get::<i64>(3).map_err(|_| HostError::Journal)? != not_null
            || row.get::<i64>(5).map_err(|_| HostError::Journal)? != primary_key
        {
            return Err(HostError::Journal);
        }
    }
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_definition(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'linux_launch_daemon_adoptions'",
            (),
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
    if normalize_sql(&actual) != normalize_sql(TABLE_SQL)
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT a.launch_id, a.endpoint, a.engine_id, a.evidence_kind, a.github_runner_id, a.runner_name, a.workflow_run_id, a.scale_set_job_id, a.actions_attempt, a.actions_job_id, a.actions_conclusion, a.docker_id, a.dind_id, a.worker_volume, a.outer_network_name, a.outer_network_id, a.adopted_at_ms, i.kind, i.state, i.effect_state, i.cleanup_proven, i.github_runner_id, i.runner_name, s.launch_id, i.observed_job_id, i.observed_workflow_run_id, i.remote_terminal, i.observed_actions_attempt, i.observed_actions_job_id, i.observed_actions_conclusion, i.docker_id, i.dind_id, i.worker_volume, i.outer_network_name, i.outer_network_id, b.endpoint, b.engine_id, c.complete FROM linux_launch_daemon_adoptions AS a LEFT JOIN intents AS i ON i.id = a.launch_id LEFT JOIN linux_launch_started_observations AS s ON s.launch_id = a.launch_id LEFT JOIN linux_launch_daemon_bindings AS b ON b.launch_id = a.launch_id LEFT JOIN worker_cleanup AS c ON c.launch_id = a.launch_id ORDER BY a.launch_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        validate_adoption_row(&row)?;
    }
    Ok(())
}

fn validate_adoption_row(row: &turso::Row) -> Result<(), HostError> {
    let adoption_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
    let endpoint = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    let engine_id = row.get::<String>(2).map_err(|_| HostError::Journal)?;
    if adoption_id <= 0
        || row.get::<String>(3).map_err(|_| HostError::Journal)?
            != "started_actions_completed_inventory_v1"
    {
        return Err(HostError::Journal);
    }
    validate_runner_evidence(row)?;
    validate_resource_evidence(row, &endpoint, &engine_id)?;
    validate_lifecycle_evidence(row, adoption_id)?;
    crate::journal::JournalDockerDaemonBinding::new(&endpoint, &engine_id)?;
    Ok(())
}

fn validate_runner_evidence(row: &turso::Row) -> Result<(), HostError> {
    if row.get::<String>(4).map_err(|_| HostError::Journal)?
        != row
            .get::<Option<String>>(21)
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
        || row.get::<String>(5).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(22)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<i64>(6).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<i64>>(25)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<String>(7).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(24)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<i64>(8).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<i64>>(27)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<i64>(9).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<i64>>(28)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row
            .get::<Option<String>>(10)
            .map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(29)
                .map_err(|_| HostError::Journal)?
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_resource_evidence(
    row: &turso::Row,
    endpoint: &str,
    engine_id: &str,
) -> Result<(), HostError> {
    if row.get::<String>(11).map_err(|_| HostError::Journal)?
        != row
            .get::<Option<String>>(30)
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
        || row.get::<String>(12).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(31)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<String>(13).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(32)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<String>(14).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(33)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row.get::<String>(15).map_err(|_| HostError::Journal)?
            != row
                .get::<Option<String>>(34)
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Journal)?
        || row
            .get::<Option<String>>(35)
            .map_err(|_| HostError::Journal)?
            != Some(endpoint.to_owned())
        || row
            .get::<Option<String>>(36)
            .map_err(|_| HostError::Journal)?
            != Some(engine_id.to_owned())
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_lifecycle_evidence(row: &turso::Row, launch_id: i64) -> Result<(), HostError> {
    validate_cleanup_state(row)?;
    if row
        .get::<Option<String>>(17)
        .map_err(|_| HostError::Journal)?
        .as_deref()
        != Some("launch")
        || row
            .get::<Option<String>>(18)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            != Some("done")
        || row
            .get::<Option<String>>(19)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            != Some("may_have_effect")
        || row.get::<Option<i64>>(23).map_err(|_| HostError::Journal)? != Some(launch_id)
        || row.get::<Option<i64>>(26).map_err(|_| HostError::Journal)? != Some(1)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_cleanup_state(row: &turso::Row) -> Result<(), HostError> {
    match (
        row.get::<Option<i64>>(20).map_err(|_| HostError::Journal)?,
        row.get::<Option<i64>>(37).map_err(|_| HostError::Journal)?,
    ) {
        (Some(0), None | Some(0)) | (Some(1), Some(1)) => Ok(()),
        _ => Err(HostError::Journal),
    }
}

fn normalize_sql(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
