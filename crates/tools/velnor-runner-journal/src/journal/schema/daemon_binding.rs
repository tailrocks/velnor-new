//! V11-to-V12 migration for Linux logical Docker Engine bindings.

use crate::error::HostError;

use super::validation;

pub(super) async fn migrate_version_eleven(conn: &turso::Connection) -> Result<(), HostError> {
    validation::validate_v11_schema(conn).await?;
    conn.execute(validation::daemon_binding::TABLE_SQL, ())
        .await
        .map_err(|_| HostError::Journal)?;
    validation::daemon_binding::validate_schema(conn).await?;
    conn.execute("PRAGMA user_version = 12", ())
        .await
        .map_err(|_| HostError::Journal)?;
    validation::v13::validate_v12_schema(conn).await
}
