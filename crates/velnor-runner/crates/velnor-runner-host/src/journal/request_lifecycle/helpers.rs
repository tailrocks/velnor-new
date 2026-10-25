//! Shared identity and digest checks for request rows.

use crate::error::HostError;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};

pub(super) async fn validate_scope_identity(
    scope: &ConnectionJournal,
    connection: &turso::Connection,
    identity: &AssignmentIdentity,
) -> Result<(), HostError> {
    scope.validate_scope(connection).await?;
    if identity.connection_id() != scope.connection_id()
        || identity.request_id().is_none()
        || bound_set_id(connection, scope.connection_id().as_str()).await?
            != identity.scale_set_id()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) async fn bound_set_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<Option<i64>>(0)
        .map_err(|_| HostError::Journal)?
        .filter(|id| *id > 0)
        .ok_or(HostError::Journal)
}

pub(super) fn request_id(
    identity: &AssignmentIdentity,
    scope: &ConnectionJournal,
) -> Result<i64, HostError> {
    identity
        .request_id()
        .filter(|request_id| identity.connection_id() == scope.connection_id() && *request_id > 0)
        .ok_or(HostError::Identity)
}
pub(super) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _written = write!(output, "{byte:02x}");
    }
    output
}
