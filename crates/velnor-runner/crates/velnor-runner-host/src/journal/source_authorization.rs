//! Durable source authorization and closed-world acquire gates.

mod model;
mod storage;
#[cfg(test)]
mod tests;

use crate::error::HostError;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::launch::steps::source::AuthorizedSourceReceipt;
use model::StoredSource;

pub(in crate::journal) fn validate_stored_source(
    source_json: &str,
    expected_repository_id: i64,
) -> Result<(), HostError> {
    StoredSource::parse(source_json, expected_repository_id).map(|_source| ())
}

/// Result of storing one immutable authenticated source receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceAuthorizationResult {
    /// The exact verified source was committed for this request.
    Recorded,
    /// The same exact receipt was already committed for this launch.
    Replayed,
}

/// Exact outcome of one claimed AcquireJobs effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignedAcquireResult {
    /// IDs from the authoritative successful response.
    Returned(Vec<i64>),
    /// The service definitively rejected the request without acquiring work.
    Rejected,
    /// The effect may have completed; no replay is authorized.
    Uncertain,
}

impl ConnectionJournal {
    /// Persist a source decision after reservation and identity binding, before acquire.
    pub(crate) async fn record_source_authorization(
        &self,
        identity: &AssignmentIdentity,
        receipt: &AuthorizedSourceReceipt,
    ) -> Result<SourceAuthorizationResult, HostError> {
        storage::record(self, identity, receipt).await
    }
}

pub(in crate::journal) async fn source_authorized_for_intent(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    intent_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    storage::matches_intent(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        intent_id,
        binding_digest,
    )
    .await
}

pub(in crate::journal) async fn created_set_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    storage::created_set_matches(connection, connection_id, scale_set_id, binding_digest).await
}

pub(in crate::journal) async fn closed_world_allows_jit(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    intent_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    storage::closed_world_allows_jit(
        connection,
        connection_id,
        scale_set_id,
        intent_id,
        binding_digest,
    )
    .await
}
