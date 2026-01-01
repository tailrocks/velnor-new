//! V12-to-V13 migration for explicit legacy launch adoption evidence.

use crate::error::HostError;

use super::validation;

pub(super) async fn migrate_version_twelve(conn: &turso::Connection) -> Result<(), HostError> {
    validation::v13::validate_v12_schema(conn).await?;
    conn.execute(validation::legacy_adoption::STARTED_TABLE_SQL, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute(validation::legacy_adoption::TABLE_SQL, ())
        .await
        .map_err(|_| HostError::Journal)?;
    conn.execute("PRAGMA user_version = 13", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validation::validate_current_schema(conn).await
}
