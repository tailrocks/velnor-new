//! Durable, append-only source-transition rows.

use crate::error::HostError;

use super::sql_normalization::normalize;

const PLAN_TABLE: &str = "CREATE TABLE IF NOT EXISTS pr_source_transitions (transition_id TEXT PRIMARY KEY CHECK (length(transition_id) = 64 AND transition_id NOT GLOB '*[^0-9a-f]*'), plan_digest TEXT NOT NULL UNIQUE CHECK (length(plan_digest) = 64 AND plan_digest NOT GLOB '*[^0-9a-f]*'), plan_json TEXT NOT NULL CHECK (length(CAST(plan_json AS BLOB)) BETWEEN 1 AND 8192), created_at INTEGER NOT NULL CHECK (created_at >= 0))";
const EVENT_TABLE: &str = "CREATE TABLE IF NOT EXISTS pr_source_transition_events (transition_id TEXT NOT NULL REFERENCES pr_source_transitions(transition_id), sequence INTEGER NOT NULL CHECK (sequence >= 0 AND sequence < 16), event_json TEXT NOT NULL CHECK (length(CAST(event_json AS BLOB)) BETWEEN 1 AND 32768), PRIMARY KEY (transition_id, sequence))";
const PLAN_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS pr_source_transitions_no_update BEFORE UPDATE ON pr_source_transitions BEGIN SELECT RAISE(ABORT, 'source transition plan is immutable'); END";
const PLAN_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS pr_source_transitions_no_delete BEFORE DELETE ON pr_source_transitions BEGIN SELECT RAISE(ABORT, 'source transition plan is retained'); END";
const EVENT_INSERT_GUARD: &str = "CREATE TRIGGER IF NOT EXISTS pr_source_transition_event_insert_guard BEFORE INSERT ON pr_source_transition_events WHEN NEW.sequence != COALESCE((SELECT MAX(sequence) + 1 FROM pr_source_transition_events WHERE transition_id = NEW.transition_id), 0) BEGIN SELECT RAISE(ABORT, 'source transition events must append in order'); END";
const EVENT_NO_UPDATE: &str = "CREATE TRIGGER IF NOT EXISTS pr_source_transition_events_no_update BEFORE UPDATE ON pr_source_transition_events BEGIN SELECT RAISE(ABORT, 'source transition event is immutable'); END";
const EVENT_NO_DELETE: &str = "CREATE TRIGGER IF NOT EXISTS pr_source_transition_events_no_delete BEFORE DELETE ON pr_source_transition_events BEGIN SELECT RAISE(ABORT, 'source transition event is retained'); END";

const OBJECTS: [(&str, &str, &str); 5] = [
    ("table", "pr_source_transitions", PLAN_TABLE),
    ("table", "pr_source_transition_events", EVENT_TABLE),
    ("trigger", "pr_source_transitions_no_update", PLAN_NO_UPDATE),
    ("trigger", "pr_source_transitions_no_delete", PLAN_NO_DELETE),
    (
        "trigger",
        "pr_source_transition_event_insert_guard",
        EVENT_INSERT_GUARD,
    ),
];
const EVENT_IMMUTABILITY: [(&str, &str); 2] = [
    ("pr_source_transition_events_no_update", EVENT_NO_UPDATE),
    ("pr_source_transition_events_no_delete", EVENT_NO_DELETE),
];

pub(super) async fn bootstrap(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, _, ddl) in OBJECTS {
        connection
            .execute(ddl, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (_, ddl) in EVENT_IMMUTABILITY {
        connection
            .execute(ddl, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate(connection).await
}

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, expected) in OBJECTS {
        validate_object(connection, kind, name, expected).await?;
    }
    for (name, expected) in EVENT_IMMUTABILITY {
        validate_object(connection, "trigger", name, expected).await?;
    }
    validate_sequences(connection).await
}

async fn validate_object(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
    expected: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if normalize(&actual) != normalize(expected)
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_sequences(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT transition_id, COUNT(*), MIN(sequence), MAX(sequence) FROM pr_source_transition_events GROUP BY transition_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let count: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let first: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let last: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        if count <= 0 || first != 0 || last + 1 != count || count > 16 {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::error::HostError;
    use crate::journal::{Journal, request_lifecycle::tests::helpers::Scratch};

    #[tokio::test]
    async fn version_37_migration_creates_transition_tables_and_guards() -> Result<(), HostError> {
        let scratch = Scratch::new()?;
        let path = scratch.file();
        let journal = Journal::open(&path).await?;
        let connection = journal.connection().await?;
        super::super::jit_conflict_recovery::remove_receipt_schema_for_migration_test(&connection)
            .await?;
        for statement in [
            "DROP TRIGGER pr_source_transitions_no_update",
            "DROP TRIGGER pr_source_transitions_no_delete",
            "DROP TRIGGER pr_source_transition_event_insert_guard",
            "DROP TRIGGER pr_source_transition_events_no_update",
            "DROP TRIGGER pr_source_transition_events_no_delete",
        ] {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
        connection
            .execute("DROP TABLE pr_source_transition_events", ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute("DROP TABLE pr_source_transitions", ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute("PRAGMA user_version = 37", ())
            .await
            .map_err(|_| HostError::Journal)?;

        super::super::driver::migrate_to_current(&connection, 37).await?;
        super::super::validate_persisted_schema_v39(&connection).await?;
        let mut rows = connection
            .query("PRAGMA user_version", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        assert_eq!(
            row.get::<i64>(0).map_err(|_| HostError::Journal)?,
            super::super::JOURNAL_VERSION
        );
        Ok(())
    }
}
