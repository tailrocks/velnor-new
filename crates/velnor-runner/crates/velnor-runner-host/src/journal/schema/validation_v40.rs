//! Explicit current-version composition; legacy validators stay exact.

use crate::error::HostError;

pub(in crate::journal) async fn validate_persisted_schema_v40(
    conn: &turso::Connection,
) -> Result<(), HostError> {
    super::legacy_validation::validate_persisted_schema_v40_base(conn).await?;
    super::jit_conflict_settlement::validate(conn).await?;
    Ok(())
}
