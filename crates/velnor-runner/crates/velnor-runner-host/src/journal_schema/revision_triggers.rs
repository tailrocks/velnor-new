use turso::Connection;

use crate::error::HostError;

pub(crate) async fn install_v18_revision_triggers(
    connection: &Connection,
) -> Result<(), HostError> {
    for statement in [
        "CREATE TRIGGER IF NOT EXISTS connection_request_reservations_revision_insert AFTER INSERT ON connection_request_reservations BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS connection_request_reservations_revision_update AFTER UPDATE ON connection_request_reservations BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS connection_request_reservations_revision_delete AFTER DELETE ON connection_request_reservations BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS doctor_probe_requests_revision_insert AFTER INSERT ON doctor_probe_requests BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS doctor_probe_requests_revision_update AFTER UPDATE ON doctor_probe_requests BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
        "CREATE TRIGGER IF NOT EXISTS doctor_probe_requests_revision_delete AFTER DELETE ON doctor_probe_requests BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
    ] {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(crate) async fn install_v20_revision_triggers(
    connection: &Connection,
) -> Result<(), HostError> {
    for statement in V20_REVISION_TRIGGERS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate_v20_revision_triggers(connection).await
}

pub(crate) async fn validate_v20_revision_triggers(
    connection: &Connection,
) -> Result<(), HostError> {
    for (name, expected) in V20_REVISION_TRIGGERS.iter().map(|statement| {
        let name = statement
            .split_once("TRIGGER IF NOT EXISTS ")
            .and_then(|(_, tail)| tail.split_once(' ').map(|(name, _)| name))
            .unwrap_or_default();
        (name, statement.replace(" IF NOT EXISTS", ""))
    }) {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [name],
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
        if normalize_trigger(&actual) != normalize_trigger(&expected) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

const V20_REVISION_TRIGGERS: [&str; 3] = [
    "CREATE TRIGGER IF NOT EXISTS storage_initialization_revision_insert AFTER INSERT ON capacity_reservation_storage_initialization BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
    "CREATE TRIGGER IF NOT EXISTS storage_initialization_revision_update AFTER UPDATE ON capacity_reservation_storage_initialization BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
    "CREATE TRIGGER IF NOT EXISTS storage_initialization_revision_delete AFTER DELETE ON capacity_reservation_storage_initialization BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
];

fn normalize_trigger(sql: &str) -> String {
    sql.to_ascii_lowercase()
        .replace("create trigger if not exists", "create trigger")
        .replace(char::is_whitespace, "")
}
