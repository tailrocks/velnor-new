//! Ordered local journal schema migrations.

use crate::error::HostError;

const JOURNAL_VERSION: i64 = 1;

pub(super) async fn ensure_columns(conn: &turso::Connection) -> Result<(), HostError> {
    let mut has_dind = false;
    let mut has_volume = false;
    let mut rows = conn
        .query("PRAGMA table_info(intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        has_dind |= name == "dind_id";
        has_volume |= name == "worker_volume";
    }
    if !has_dind {
        conn.execute("ALTER TABLE intents ADD COLUMN dind_id TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    if !has_volume {
        conn.execute("ALTER TABLE intents ADD COLUMN worker_volume TEXT", ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn migrate_legacy_launch_failures(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = migrate_version_zero(conn).await;
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

async fn migrate_version_zero(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let version = row
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    match version {
        0 => {
            conn.execute(
                "UPDATE intents SET state = 'uncertain', cleanup_proven = 0 WHERE kind = 'launch' AND state = 'failed'",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
            conn.execute("PRAGMA user_version = 1", ())
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        }
        JOURNAL_VERSION => Ok(()),
        _ => Err(HostError::Journal),
    }
}
