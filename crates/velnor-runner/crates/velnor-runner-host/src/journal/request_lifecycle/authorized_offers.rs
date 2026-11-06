// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ConnectionId, SourcePolicyMode};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::read_transaction;

use crate::journal::source_authorization::{
    created_set_matches, encode_source_receipt, validate_stored_source,
};
use crate::launch::steps::source::AuthorizedSourceReceipt;

use super::helpers::{bound_set_id, digest};

const AUTHORITY_MODEL: &str = "created_empty_exclusive_admin_session_v1";
const MAX_AUTHORIZED_OFFER_SCAN: u32 = 256;
const MAX_SOURCE_JSON_BYTES: usize = 32_768;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AuthorizedOffer {
    connection_id: ConnectionId,
    scale_set_id: i64,
    request_id: i64,
    message_id: i64,
    policy_mode: SourcePolicyMode,
    binding_digest: String,
    source_digest: String,
    source_json: String,
}

impl AuthorizedOffer {
    #[must_use]
    pub(crate) fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    #[must_use]
    pub(crate) const fn request_id(&self) -> i64 {
        self.request_id
    }

    #[must_use]
    pub(crate) const fn policy_mode(&self) -> SourcePolicyMode {
        self.policy_mode
    }

    #[must_use]
    pub(crate) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    #[must_use]
    pub(crate) fn source_json(&self) -> &str {
        &self.source_json
    }
}

impl ConnectionJournal {
    pub(crate) async fn authorized_offers(
        &self,
        limit: u32,
    ) -> Result<Vec<AuthorizedOffer>, HostError> {
        if limit == 0 || limit > MAX_AUTHORIZED_OFFER_SCAN {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = bound_set_id(connection, &connection_id).await?;
            if !created_set_matches(
                connection,
                &connection_id,
                scale_set_id,
                &binding_digest,
            )
            .await?
            {
                return Err(HostError::Journal);
            }
            let repository_id = repository_id(connection, &connection_id).await?;
            let mut rows = connection
                .query(
                    "SELECT request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND binding_digest = ?3 AND authority_model = ?4 AND acquire_state = 'authorized' AND started_event_digest IS NULL AND completed_event_digest IS NULL ORDER BY message_id, request_id LIMIT ?5",
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
            let mut offers = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                offers.push(parse_offer(
                    self.connection_id(),
                    scale_set_id,
                    &row,
                    &binding_digest,
                    repository_id,
                )?);
                drop(row);
            }
            Ok(offers)
        })
        .await
    }

    pub(crate) async fn authorized_offer_for_receipt(
        &self,
        receipt: &AuthorizedSourceReceipt,
    ) -> Result<Option<AuthorizedOffer>, HostError> {
        if receipt.connection_id() != self.connection_id()
            || receipt.binding_digest() != self.binding_digest()
            || receipt.request_id() <= 0
            || receipt.message_id() < 0
        {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        let binding_digest = self.binding_digest().to_owned();
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let scale_set_id = bound_set_id(connection, &connection_id).await?;
            if receipt.scale_set_id() != scale_set_id
                || !created_set_matches(
                    connection,
                    &connection_id,
                    scale_set_id,
                    &binding_digest,
                )
                .await?
            {
                return Err(HostError::Journal);
            }
            let repository_id = repository_id(connection, &connection_id).await?;
            let expected_source = encode_source_receipt(receipt, repository_id)?;
            let mut rows = connection
                .query(
                    "SELECT request_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3 AND binding_digest = ?4 AND authority_model = ?5 AND acquire_state = 'authorized' AND started_event_digest IS NULL AND completed_event_digest IS NULL",
                    (
                        connection_id.as_str(),
                        scale_set_id,
                        receipt.request_id(),
                        binding_digest.as_str(),
                        AUTHORITY_MODEL,
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let offer = parse_offer(
                self.connection_id(),
                scale_set_id,
                &row,
                &binding_digest,
                repository_id,
            )?;
            drop(row);
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            if offer.source_json() != expected_source
                || offer.policy_mode() != receipt.policy_mode()
                || offer.request_id() != receipt.request_id()
            {
                return Err(HostError::Identity);
            }
            Ok(Some(offer))
        })
        .await
    }
}

fn parse_offer(
    connection_id: &ConnectionId,
    scale_set_id: i64,
    row: &turso::Row,
    expected_binding_digest: &str,
    repository_id: i64,
) -> Result<AuthorizedOffer, HostError> {
    let request_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let message_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let policy_mode = parse_policy_mode(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
    let authority_model: String = row.get(3).map_err(|_| HostError::Journal)?;
    let binding_digest: String = row.get(4).map_err(|_| HostError::Journal)?;
    let source_digest: String = row.get(5).map_err(|_| HostError::Journal)?;
    let source_json: String = row.get(6).map_err(|_| HostError::Journal)?;
    if request_id <= 0
        || message_id < 0
        || authority_model != AUTHORITY_MODEL
        || binding_digest != expected_binding_digest
        || source_json.is_empty()
        || source_json.len() > MAX_SOURCE_JSON_BYTES
        || digest(source_json.as_bytes()) != source_digest
    {
        return Err(HostError::Journal);
    }
    validate_stored_source(&source_json, repository_id)?;
    Ok(AuthorizedOffer {
        connection_id: connection_id.clone(),
        scale_set_id,
        request_id,
        message_id,
        policy_mode,
        binding_digest,
        source_digest,
        source_json,
    })
}

fn parse_policy_mode(value: &str) -> Result<SourcePolicyMode, HostError> {
    match value {
        "repository" => Ok(SourcePolicyMode::Repository),
        "branch_restricted" => Ok(SourcePolicyMode::BranchRestricted),
        _ => Err(HostError::Journal),
    }
}

async fn repository_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1 AND retired = 0",
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
