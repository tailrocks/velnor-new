//! One-shot `AcquireJobs` state transitions and request lookup.

use serde::Serialize;

use crate::error::HostError;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::connection::{AssignmentIdentity, AssignmentKey, ConnectionJournal};
use crate::journal::source_authorization::AssignedAcquireResult;

use super::RequestAcquireState;
use super::helpers::{bound_set_id, digest, request_id, validate_scope_identity};
use crate::journal::source_authorization::created_set_matches;

const AUTHORITY_MODEL: &str = "created_empty_exclusive_admin_session_v1";
const MAX_ACQUIRE_RESPONSE_BYTES: usize = 4_096;
const MAX_ACQUIRE_SCAN: u32 = 4_096;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AcquireResponse<'a> {
    Returned { request_ids: &'a [i64] },
    NoEffect,
    AlreadyAcquired,
    Rejected,
    Uncertain,
}

impl ConnectionJournal {
    /// Record the one-shot `AcquireJobs` intent before the HTTP request.
    pub(crate) async fn claim_request_acquire(
        &self,
        identity: &AssignmentIdentity,
    ) -> Result<bool, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            claim_request_acquire_in_transaction(connection, self, identity).await
        })
        .await
    }

    /// Record the exact `AcquireJobs` outcome; non-exact IDs remain uncertain.
    pub(crate) async fn record_request_acquire(
        &self,
        identity: &AssignmentIdentity,
        result: AssignedAcquireResult,
    ) -> Result<(), HostError> {
        let request_id = request_id(identity, self)?;
        let (state, response_json) = response_record(&result, request_id)?;
        let response_digest = digest(response_json.as_bytes());
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            validate_scope_identity(self, connection, identity).await?;
            let changed = connection
                .execute(
                    "UPDATE connection_request_authorizations SET acquire_state = ?1, acquire_response_json = ?2, acquire_response_digest = ?3 WHERE connection_id = ?4 AND scale_set_id = ?5 AND request_id = ?6 AND binding_digest = ?7 AND acquire_state = 'requested' AND acquire_response_json IS NULL AND acquire_response_digest IS NULL",
                    (
                        state,
                        response_json.as_str(),
                        response_digest.as_str(),
                        connection_id.as_str(),
                        identity.scale_set_id(),
                        request_id,
                        binding_digest.as_str(),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1
                || acquire_result_matches(
                    connection,
                    &connection_id,
                    identity.scale_set_id(),
                    request_id,
                    state,
                    &response_digest,
                )
                .await?
            {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// Read the durable `AcquireJobs` state for one scoped request.
    pub(crate) async fn request_acquire_state(
        &self,
        identity: &AssignmentIdentity,
    ) -> Result<Option<RequestAcquireState>, HostError> {
        let request_id = request_id(identity, self)?;
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            validate_scope_identity(self, connection, identity).await?;
            let mut rows = connection
                .query(
                    "SELECT acquire_state, binding_digest FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
                    (connection_id.as_str(), identity.scale_set_id(), request_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let state: String = row.get(0).map_err(|_| HostError::Journal)?;
            let stored_binding: String = row.get(1).map_err(|_| HostError::Journal)?;
            if stored_binding != binding_digest {
                return Err(HostError::Journal);
            }
            parse_state(&state).map(Some)
        })
        .await
    }

    /// Give acquired, authorized requests with no start or completion event.
    pub(crate) async fn acquired_authorized_requests(
        &self,
        limit: u32,
    ) -> Result<Vec<AssignmentIdentity>, HostError> {
        if limit == 0 || limit > MAX_ACQUIRE_SCAN {
            return Err(HostError::Journal);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = bound_set_id(connection, &connection_id).await?;
            let mut rows = connection
                .query(
                    "SELECT request_id FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND binding_digest = ?3 AND authority_model = ?4 AND acquire_state = 'acquired' AND started_event_digest IS NULL AND completed_event_digest IS NULL ORDER BY request_id LIMIT ?5",
                    (
                        connection_id.as_str(),
                        scale_set_id,
                        binding_digest.as_str(),
                        AUTHORITY_MODEL,
                        i64::from(limit),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut identities = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                let request_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
                identities.push(AssignmentIdentity::new(
                    self.connection_id(),
                    scale_set_id,
                    AssignmentKey::AssignedRequest { request_id },
                )?);
            }
            Ok(identities)
        })
        .await
    }
}

/// Record permission for one `AcquireJobs` call in the caller's database transaction.
pub(in crate::journal) async fn claim_request_acquire_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    identity: &AssignmentIdentity,
) -> Result<bool, HostError> {
    let request_id = request_id(identity, scope)?;
    let connection_id = scope.connection_id().as_str();
    let binding_digest = scope.binding_digest();
    validate_scope_identity(scope, connection, identity).await?;
    if !created_set_matches(
        connection,
        connection_id,
        identity.scale_set_id(),
        binding_digest,
    )
    .await?
    {
        return Ok(false);
    }
    let changed = connection
        .execute(
            "UPDATE connection_request_authorizations SET acquire_state = 'requested' WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3 AND binding_digest = ?4 AND authority_model = ?5 AND acquire_state = 'authorized' AND started_event_digest IS NULL AND completed_event_digest IS NULL",
            (
                connection_id,
                identity.scale_set_id(),
                request_id,
                binding_digest,
                AUTHORITY_MODEL,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(changed == 1)
}

fn response_record(
    result: &AssignedAcquireResult,
    request_id: i64,
) -> Result<(&'static str, String), HostError> {
    let (state, response) = match result {
        AssignedAcquireResult::Returned(ids) if ids.as_slice() == [request_id] => {
            ("acquired", AcquireResponse::Returned { request_ids: ids })
        }
        AssignedAcquireResult::Returned(ids) => {
            ("uncertain", AcquireResponse::Returned { request_ids: ids })
        }
        AssignedAcquireResult::NoEffect => ("no_effect", AcquireResponse::NoEffect),
        AssignedAcquireResult::AlreadyAcquired => {
            ("already_acquired", AcquireResponse::AlreadyAcquired)
        }
        AssignedAcquireResult::Rejected => ("rejected", AcquireResponse::Rejected),
        AssignedAcquireResult::Uncertain => ("uncertain", AcquireResponse::Uncertain),
    };
    let json = serde_json::to_string(&response).map_err(|_| HostError::Journal)?;
    if json.len() > MAX_ACQUIRE_RESPONSE_BYTES {
        return Err(HostError::Journal);
    }
    Ok((state, json))
}

pub(super) async fn acquired_backlog_is_closed(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT request_id, policy_mode, authority_model, binding_digest, source_digest, source_json, acquire_response_json, acquire_response_digest FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND acquire_state = 'acquired' ORDER BY request_id LIMIT ?3",
            (connection_id, scale_set_id, i64::from(MAX_ACQUIRE_SCAN) + 1),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let (repository_id, policy_mode) =
        bound_source_identity(connection, connection_id, binding_digest).await?;
    let mut seen = 0u32;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        seen += 1;
        if seen > MAX_ACQUIRE_SCAN {
            return Ok(false);
        }
        let request_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let stored_mode: String = row.get(1).map_err(|_| HostError::Journal)?;
        let authority: String = row.get(2).map_err(|_| HostError::Journal)?;
        let stored_binding: String = row.get(3).map_err(|_| HostError::Journal)?;
        let source_digest: String = row.get(4).map_err(|_| HostError::Journal)?;
        let source_json: String = row.get(5).map_err(|_| HostError::Journal)?;
        let response_json: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
        let response_digest: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
        let Some(response_json) = response_json else {
            return Ok(false);
        };
        let Some(response_digest) = response_digest else {
            return Ok(false);
        };
        let expected_response = serde_json::to_string(&AcquireResponse::Returned {
            request_ids: &[request_id],
        })
        .map_err(|_| HostError::Journal)?;
        if request_id <= 0
            || stored_mode != policy_mode
            || authority != AUTHORITY_MODEL
            || stored_binding != binding_digest
            || !valid_digest(&source_digest)
            || digest(source_json.as_bytes()) != source_digest
            || crate::journal::source_authorization::validate_stored_source(
                &source_json,
                repository_id,
            )
            .is_err()
            || response_json != expected_response
            || digest(response_json.as_bytes()) != response_digest
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn bound_source_identity(
    connection: &turso::Connection,
    connection_id: &str,
    binding_digest: &str,
) -> Result<(i64, String), HostError> {
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
    if digest(identity.as_bytes()) != binding_digest {
        return Err(HostError::Journal);
    }
    let identity: serde_json::Value =
        serde_json::from_str(&identity).map_err(|_| HostError::Journal)?;
    let repository_id = identity
        .get("repository_id")
        .and_then(serde_json::Value::as_i64)
        .filter(|id| *id > 0)
        .ok_or(HostError::Journal)?;
    let policy_mode = identity
        .get("source_policy")
        .and_then(|policy| policy.get("mode"))
        .and_then(serde_json::Value::as_str)
        .filter(|mode| matches!(*mode, "repository" | "branch_restricted"))
        .ok_or(HostError::Journal)?
        .to_owned();
    Ok((repository_id, policy_mode))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn acquire_result_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    state: &str,
    response_digest: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT acquire_state, acquire_response_digest FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_digest: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    Ok(stored_state == state && stored_digest.as_deref() == Some(response_digest))
}

pub(in crate::journal) fn parse_state(value: &str) -> Result<RequestAcquireState, HostError> {
    match value {
        "authorized" => Ok(RequestAcquireState::Authorized),
        "requested" => Ok(RequestAcquireState::Requested),
        "acquired" => Ok(RequestAcquireState::Acquired),
        "no_effect" => Ok(RequestAcquireState::NoEffect),
        "already_acquired" => Ok(RequestAcquireState::AlreadyAcquired),
        "rejected" => Ok(RequestAcquireState::Rejected),
        "uncertain" => Ok(RequestAcquireState::Uncertain),
        _ => Err(HostError::Journal),
    }
}
