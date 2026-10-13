//! Validation for the V12 daemon binding and V13 legacy-adoption tables.

use crate::error::HostError;

use super::{daemon_binding, legacy_adoption, validate_v11_schema};

pub(in crate::journal::schema) async fn validate_v12_schema(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    validate_v11_schema(conn).await?;
    daemon_binding::validate_schema(conn).await
}

pub(super) async fn validate_current_schema(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v12_schema(conn).await?;
    legacy_adoption::validate_started_schema(conn).await?;
    legacy_adoption::validate_schema(conn).await
}
