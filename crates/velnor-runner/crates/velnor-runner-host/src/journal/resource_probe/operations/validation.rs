//! Check probe operations against their journal identity and lease state.

use crate::error::HostError;
use crate::journal::ProbeSeed;

pub(in crate::journal) async fn validate_binding(
    connection: &turso::Connection,
    seed: &ProbeSeed,
) -> Result<(), HostError> {
    if crate::journal_schema::instance_id(connection).await? != seed.instance_id
        || crate::journal_schema::engine_id(connection).await? != seed.engine_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(in crate::journal) async fn ensure_no_active(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_id FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') UNION ALL SELECT CAST(request_id AS TEXT) FROM doctor_probe_requests WHERE state IN ('queued', 'running', 'recovering', 'uncertain') LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(in crate::journal) async fn ensure_no_active_probe(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    ensure_no_active(connection).await
}
