//! Immutable authenticated source receipt storage.

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::journal::source_authorization::{
    SourceAuthorizationResult, created_set_matches, encode_source_receipt, source_policy_mode_name,
};
use crate::launch::steps::source::AuthorizedSourceReceipt;

use super::helpers::{digest, request_id, validate_scope_identity};

const AUTHORITY_MODEL: &str = "created_empty_exclusive_admin_session_v1";
const MAX_REQUEST_ROWS: i64 = 100_000;

impl ConnectionJournal {
    /// Record an authenticated, policy-approved request before acquiring it.
    pub(crate) async fn record_request_source(
        &self,
        identity: &AssignmentIdentity,
        receipt: &AuthorizedSourceReceipt,
    ) -> Result<SourceAuthorizationResult, HostError> {
        let request_id = request_id(identity, self)?;
        if receipt.request_id() != request_id
            || receipt.message_id() < 0
            || receipt.connection_id() != self.connection_id()
            || receipt.scale_set_id() != identity.scale_set_id()
            || receipt.binding_digest() != self.binding_digest()
        {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            validate_scope_identity(self, connection, identity).await?;
            if !created_set_matches(
                connection,
                &connection_id,
                identity.scale_set_id(),
                &binding_digest,
            )
            .await?
            {
                return Err(HostError::Journal);
            }
            let repository_id = repository_id(connection, &connection_id).await?;
            let source_json = encode_source_receipt(receipt, repository_id)?;
            if source_json.is_empty() || source_json.len() > 32_768 {
                return Err(HostError::Identity);
            }
            let source_digest = digest(source_json.as_bytes());
            let policy_mode = source_policy_mode_name(receipt.policy_mode());
            if let Some(existing) = request_row(
                connection,
                &connection_id,
                identity.scale_set_id(),
                request_id,
            )
            .await?
            {
                return if existing.matches_receipt(
                    policy_mode,
                    &binding_digest,
                    &source_digest,
                    &source_json,
                ) {
                    Ok(SourceAuthorizationResult::Replayed)
                } else {
                    Err(HostError::Identity)
                };
            }
            if count_rows(connection).await? >= MAX_REQUEST_ROWS {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT INTO connection_request_authorizations (connection_id, scale_set_id, request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'authorized')",
                    (
                        connection_id.as_str(),
                        identity.scale_set_id(),
                        request_id,
                        receipt.message_id(),
                        policy_mode,
                        AUTHORITY_MODEL,
                        binding_digest.as_str(),
                        source_digest.as_str(),
                        source_json.as_str(),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(SourceAuthorizationResult::Recorded)
        })
        .await
    }
}

struct RequestRow {
    policy_mode: String,
    authority_model: String,
    binding_digest: String,
    source_digest: String,
    source_json: String,
}

impl RequestRow {
    fn matches_receipt(
        &self,
        policy_mode: &str,
        binding_digest: &str,
        source_digest: &str,
        source_json: &str,
    ) -> bool {
        self.policy_mode == policy_mode
            && self.authority_model == AUTHORITY_MODEL
            && self.binding_digest == binding_digest
            && self.source_digest == source_digest
            && self.source_json == source_json
    }
}

async fn request_row(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
) -> Result<Option<RequestRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT policy_mode, authority_model, binding_digest, source_digest, source_json FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(RequestRow {
        policy_mode: row.get(0).map_err(|_| HostError::Journal)?,
        authority_model: row.get(1).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(2).map_err(|_| HostError::Journal)?,
        source_digest: row.get(3).map_err(|_| HostError::Journal)?,
        source_json: row.get(4).map_err(|_| HostError::Journal)?,
    }))
}

async fn count_rows(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("SELECT COUNT(*) FROM connection_request_authorizations", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn repository_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let identity: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    serde_json::from_str::<serde_json::Value>(&identity)
        .map_err(|_| HostError::Journal)?
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|id| *id > 0)
        .ok_or(HostError::Journal)
}
