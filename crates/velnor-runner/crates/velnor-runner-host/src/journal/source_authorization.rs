// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod model;
mod storage;

use crate::error::HostError;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::launch::steps::source::AuthorizedSourceReceipt;
pub(in crate::journal) use model::EventSource;
use model::StoredSource;

pub(in crate::journal) fn validate_stored_source(
    source_json: &str,
    expected_repository_id: i64,
) -> Result<(), HostError> {
    StoredSource::parse(source_json, expected_repository_id).map(|_source| ())
}

pub(in crate::journal) fn stored_source_already_assigned(
    source_json: &str,
    expected_repository_id: i64,
) -> Result<bool, HostError> {
    StoredSource::parse(source_json, expected_repository_id).map(|source| source.already_assigned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceAuthorizationResult {
    Recorded,

    Replayed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignedAcquireResult {
    Returned(Vec<i64>),

    NoEffect,

    AlreadyAcquired,

    Rejected,

    Uncertain,
}

impl ConnectionJournal {
    pub(crate) async fn record_source_authorization(
        &self,
        identity: &AssignmentIdentity,
        receipt: &AuthorizedSourceReceipt,
    ) -> Result<SourceAuthorizationResult, HostError> {
        self.record_request_source(identity, receipt).await
    }
}

pub(in crate::journal) fn encode_source_receipt(
    receipt: &AuthorizedSourceReceipt,
    repository_id: i64,
) -> Result<String, HostError> {
    StoredSource::from_receipt(receipt, repository_id)?.json()
}

pub(in crate::journal) fn source_policy_mode_name(
    mode: crate::config::SourcePolicyMode,
) -> &'static str {
    match mode {
        crate::config::SourcePolicyMode::Repository => "repository",
        crate::config::SourcePolicyMode::BranchRestricted => "branch_restricted",
    }
}

pub(in crate::journal) async fn event_source_matches(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    binding_digest: &str,
    event: EventSource<'_>,
) -> Result<bool, HostError> {
    storage::event_source_matches(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        binding_digest,
        event,
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
