//! Lineage-guarded source receipt writes and acquire admission queries.

mod records;

use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::journal::source_authorization::EventSource;
use crate::journal::source_authorization::model::StoredSource;

const AUTHORITY_MODEL: &str = "created_empty_exclusive_admin_session_v1";

pub(super) async fn event_source_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    event: EventSource<'_>,
) -> Result<bool, HostError> {
    if scale_set_id <= 0 || request_id <= 0 || !valid_digest(binding_digest) {
        return Ok(false);
    }
    let mut rows = connection
        .query(
            "SELECT authority_model, binding_digest, source_digest, source_json FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let authority_model: String = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_binding: String = row.get(1).map_err(|_| HostError::Journal)?;
    let source_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
    let source_json: String = row.get(3).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || authority_model != AUTHORITY_MODEL
        || stored_binding != binding_digest
        || !valid_digest(&source_digest)
        || digest(source_json.as_bytes()) != source_digest
        || connection_identity_digest(connection, connection_id).await? != binding_digest
    {
        return Ok(false);
    }
    let repository_id = records::repository_id(connection, connection_id).await?;
    let source = StoredSource::parse(&source_json, repository_id)?;
    Ok(source.matches_event_source(event))
}

async fn connection_identity_digest(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<String, HostError> {
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
    Ok(digest(identity.as_bytes()))
}

pub(super) async fn created_set_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    if scale_set_id <= 0 || !valid_digest(binding_digest) {
        return Ok(false);
    }
    records::created_set_count(connection, connection_id, scale_set_id, binding_digest).await
}

fn digest(value: &[u8]) -> String {
    let bytes = Sha256::digest(value);
    let mut result = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _written = write!(result, "{byte:02x}");
    }
    result
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
