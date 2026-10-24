//! Version-40 migration and tamper coverage.

#[path = "tests/evidence_tamper.rs"]
mod evidence_tamper;
#[path = "tests/fixtures.rs"]
mod fixtures;
#[path = "tests/migration.rs"]
mod migration;
#[path = "tests/tamper.rs"]
mod tamper;

pub(super) use fixtures::{
    LocalAbsenceFixture, local_absence_fixture, settle_empty_remote_case, verified_empty_lookup,
};
pub(in crate::journal::schema) use migration::downgrade_current_database_to_v39;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::HostError;
use crate::journal::schema::{journal_version, migration as schema_migration};

pub(super) const EXTERNAL_JIT_TRIGGER_NAMES: [&str; 8] = [
    "execution_proof_approval_jit_guard",
    "execution_proof_insert_jit_guard",
    "execution_publication_context_insert_guard",
    "physical_attempt_jit_conflict_receipt_insert_guard",
    "physical_attempt_outcome_insert_guard",
    "physical_attempt_post_jit_retirement_claim_insert_guard",
    "physical_attempt_post_jit_retirement_insert_guard",
    "reservation_network_transition",
];

pub(super) const TERMINAL_VIEW_TRIGGER_NAMES: [&str; 2] = [
    "physical_attempt_current_delete_guard",
    "physical_attempt_insert_guard",
];

pub(super) struct V39Fixture {
    connection: turso::Connection,
    database: turso::Database,
    database_path: PathBuf,
    scratch: Scratch,
}

impl V39Fixture {
    pub(super) const fn connection(&self) -> &turso::Connection {
        &self.connection
    }

    pub(super) fn path(&self) -> &Path {
        &self.database_path
    }

    pub(super) fn close(self) -> Scratch {
        let Self {
            connection,
            database,
            database_path: _,
            scratch,
        } = self;
        drop(connection);
        drop(database);
        scratch
    }
}

pub(super) async fn external_jit_triggers(
    connection: &turso::Connection,
) -> Result<Vec<(String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name!='connection_runner_jits' AND instr(lower(sql),'connection_runner_jits')>0 ORDER BY name",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut triggers = Vec::with_capacity(EXTERNAL_JIT_TRIGGER_NAMES.len());
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        triggers.push((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
        ));
    }
    Ok(triggers)
}

pub(super) fn has_expected_external_jit_trigger_names(triggers: &[(String, String)]) -> bool {
    triggers.len() == EXTERNAL_JIT_TRIGGER_NAMES.len()
        && triggers
            .iter()
            .map(|(name, _)| name.as_str())
            .eq(EXTERNAL_JIT_TRIGGER_NAMES)
}

pub(super) async fn terminal_view_triggers(
    connection: &turso::Connection,
) -> Result<Vec<(String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND instr(lower(sql),'physical_attempt_terminal_proofs')>0 ORDER BY name",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut triggers = Vec::with_capacity(TERMINAL_VIEW_TRIGGER_NAMES.len());
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        triggers.push((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
        ));
    }
    Ok(triggers)
}

fn has_expected_terminal_view_trigger_names(triggers: &[(String, String)]) -> bool {
    triggers.len() == TERMINAL_VIEW_TRIGGER_NAMES.len()
        && triggers
            .iter()
            .map(|(name, _)| name.as_str())
            .eq(TERMINAL_VIEW_TRIGGER_NAMES)
}

pub(super) async fn suspend_terminal_view_triggers_for_downgrade(
    legacy: &turso::Connection,
    current_connection: &turso::Connection,
) -> Result<Vec<String>, HostError> {
    let legacy = terminal_view_triggers(legacy).await?;
    let current = terminal_view_triggers(current_connection).await?;
    if !has_expected_terminal_view_trigger_names(&legacy)
        || !has_expected_terminal_view_trigger_names(&current)
    {
        return Err(HostError::Journal);
    }
    for name in TERMINAL_VIEW_TRIGGER_NAMES {
        current_connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(legacy.into_iter().map(|(_, sql)| sql).collect())
}

pub(super) async fn suspend_external_jit_triggers_for_downgrade(
    legacy: &turso::Connection,
    current_connection: &turso::Connection,
) -> Result<Vec<String>, HostError> {
    let legacy = external_jit_triggers(legacy).await?;
    let current = external_jit_triggers(current_connection).await?;
    if !has_expected_external_jit_trigger_names(&legacy)
        || !has_expected_external_jit_trigger_names(&current)
    {
        return Err(HostError::Journal);
    }
    for name in EXTERNAL_JIT_TRIGGER_NAMES {
        current_connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(legacy.into_iter().map(|(_, sql)| sql).collect())
}

pub(super) async fn object_sql(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2 AND sql IS NOT NULL",
            (kind, name),
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

pub(super) async fn attached_sql(
    connection: &turso::Connection,
    kind: &str,
    table: &str,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type=?1 AND tbl_name=?2 AND sql IS NOT NULL ORDER BY name",
            (kind, table),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut statements = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        statements.push(row.get(0).map_err(|_| HostError::Journal)?);
    }
    Ok(statements)
}

pub(super) async fn restore_external_jit_triggers(
    connection: &turso::Connection,
    triggers: &[String],
) -> Result<(), HostError> {
    for sql in triggers {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn restore_terminal_view_triggers(
    connection: &turso::Connection,
    triggers: &[String],
) -> Result<(), HostError> {
    for sql in triggers {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn replace_jit_table_for_downgrade(
    connection: &turso::Connection,
    v39_table_sql: &str,
) -> Result<(), HostError> {
    let old = "CREATE TABLE connection_runner_jits ";
    if v39_table_sql.matches(old).count() != 1 {
        return Err(HostError::Journal);
    }
    let replacement = v39_table_sql.replacen(old, "CREATE TABLE connection_runner_jits_v39 ", 1);
    connection
        .execute(&replacement, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_runner_jits_v39 (intent_id,connection_id,scale_set_id,witness_request_id,requested_name,state,runner_id,runner_name,started_request_id,completed_request_id) SELECT intent_id,connection_id,scale_set_id,witness_request_id,requested_name,state,runner_id,runner_name,started_request_id,completed_request_id FROM connection_runner_jits",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop_downgrade_object(connection, "TABLE", "connection_runner_jits").await?;
    connection
        .execute(
            "ALTER TABLE connection_runner_jits_v39 RENAME TO connection_runner_jits",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn drop_downgrade_object(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<(), HostError> {
    connection
        .execute(&format!("DROP {kind} {name}"), ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

pub(super) async fn v39_fixture() -> Result<V39Fixture, HostError> {
    let scratch = Scratch::new();
    let path = scratch.0.join("journal.db");
    crate::journal::schema::historical_fixture::install(&path, 31).await?;
    let path_text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for version in 31..=38 {
        match version {
            31 => schema_migration::from_version_thirty_one(&connection).await?,
            32 => schema_migration::from_version_thirty_two(&connection).await?,
            33 => schema_migration::from_version_thirty_three(&connection).await?,
            34 => schema_migration::from_version_thirty_four(&connection).await?,
            35 => schema_migration::from_version_thirty_five(&connection).await?,
            36 => schema_migration::from_version_thirty_six(&connection).await?,
            37 => schema_migration::from_version_thirty_seven(&connection).await?,
            38 => schema_migration::from_version_thirty_eight(&connection).await?,
            _ => return Err(HostError::Journal),
        }
    }
    let version = journal_version(&connection).await?;
    if version != 39 {
        return Err(HostError::Journal);
    }
    Ok(V39Fixture {
        connection,
        database,
        database_path: path,
        scratch,
    })
}

pub(super) struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-jit-conflict-v39-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}
