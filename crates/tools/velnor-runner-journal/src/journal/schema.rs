//! Versioned journal initialization and conservative legacy-row migration.

use crate::error::HostError;

const JOURNAL_VERSION: i64 = 13;

mod daemon_binding;
mod legacy;
mod legacy_adoption;
mod validation;
pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = bootstrap_transaction(conn).await;
    let end = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    end.map_err(|_| HostError::Journal)?;
    result
}

async fn bootstrap_transaction(conn: &turso::Connection) -> Result<(), HostError> {
    let version = journal_version(conn).await?;
    if version > JOURNAL_VERSION {
        return Err(unsupported_journal_version(version));
    }
    if version == JOURNAL_VERSION {
        return validation::validate_current_schema(conn).await;
    }
    legacy::migrate_through_v10(conn, version).await?;
    if version <= 11 {
        daemon_binding::migrate_version_eleven(conn).await?;
    }
    if version <= 12 {
        legacy_adoption::migrate_version_twelve(conn).await?;
    }
    Ok(())
}

/// Reject future schemas before an existing-only open returns a journal handle.
/// Older versions remain available to existing-only operations as before; the
/// bootstrap path is responsible for their migration.
pub(super) async fn validate_existing_version(conn: &turso::Connection) -> Result<(), HostError> {
    let version = journal_version(conn).await?;
    if version < 0 {
        return Err(HostError::Journal);
    }
    if version > JOURNAL_VERSION {
        return Err(unsupported_journal_version(version));
    }
    Ok(())
}

fn unsupported_journal_version(found: i64) -> HostError {
    HostError::UnsupportedJournalVersion {
        found,
        supported: JOURNAL_VERSION,
    }
}

async fn journal_version(conn: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}
