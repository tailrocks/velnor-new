//! Lineage-guarded source receipt writes and acquire admission queries.

mod records;

use sha2::{Digest, Sha256};

use crate::config::SourcePolicyMode;
use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::journal::source_authorization::SourceAuthorizationResult;
use crate::journal::source_authorization::model::StoredSource;
use crate::launch::steps::source::AuthorizedSourceReceipt;

const AUTHORITY_MODEL: &str = "created_empty_exclusive_admin_session_v1";
const MAX_SOURCE_ROWS: i64 = 100_000;

use records::{LaunchRow, SourceRow};

pub(super) async fn record(
    scope: &ConnectionJournal,
    identity: &AssignmentIdentity,
    receipt: &AuthorizedSourceReceipt,
) -> Result<SourceAuthorizationResult, HostError> {
    let Some(request_id) = identity.request_id() else {
        return Err(HostError::Identity);
    };
    if identity.connection_id() != scope.connection_id()
        || receipt.request_id() != request_id
        || receipt.message_id() < 0
    {
        return Err(HostError::Identity);
    }
    let connection_id = scope.connection_id().as_str().to_owned();
    let binding_digest = scope.binding_digest().to_owned();
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        require_current_set(connection, &connection_id, identity.scale_set_id()).await?;
        require_created_set(connection, &connection_id, identity.scale_set_id(), &binding_digest)
            .await?;
        let repository_id = records::repository_id(connection, &connection_id).await?;
        let source = StoredSource::from_receipt(receipt, repository_id)?;
        let source_json = source.json()?;
        if source_json.is_empty() || source_json.len() > 32_768 {
            return Err(HostError::Identity);
        }
        let source_digest = digest(source_json.as_bytes());
        let policy_mode = policy_mode(receipt.policy_mode());
        let launch = records::launch_row(connection, identity, &connection_id).await?;
        if let Some(existing) = records::source_row(connection, &connection_id, identity.scale_set_id(), request_id).await? {
            return compare_replay(
                existing,
                &launch,
                receipt,
                &binding_digest,
                &source_digest,
                &source_json,
                policy_mode,
            );
        }
        require_fresh_source_target(&launch, request_id, &binding_digest)?;
        let total = records::scalar_count(connection, "SELECT COUNT(*) FROM connection_source_authorizations").await?;
        if total >= MAX_SOURCE_ROWS {
            return Err(HostError::Journal);
        }
        connection
            .execute(
                "INSERT INTO connection_source_authorizations (connection_id, scale_set_id, request_id, intent_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (
                    connection_id.as_str(),
                    identity.scale_set_id(),
                    request_id,
                    launch.id,
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

pub(super) async fn matches_intent(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    intent_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    if scale_set_id <= 0 || request_id <= 0 || intent_id <= 0 {
        return Ok(false);
    }
    let Some(record) =
        records::source_row(connection, connection_id, scale_set_id, request_id).await?
    else {
        return Ok(false);
    };
    if record.intent_id != intent_id
        || record.authority_model != AUTHORITY_MODEL
        || record.binding_digest != binding_digest
        || !valid_digest(&record.binding_digest)
        || !valid_digest(&record.source_digest)
    {
        return Ok(false);
    }
    if !records::matching_launch_intent(
        connection,
        connection_id,
        scale_set_id,
        request_id,
        intent_id,
    )
    .await?
    {
        return Ok(false);
    }
    let repository_id = records::repository_id(connection, connection_id).await?;
    let source = StoredSource::parse(&record.source_json, repository_id)?;
    if digest(record.source_json.as_bytes()) != record.source_digest {
        return Err(HostError::Journal);
    }
    let _source = source;
    Ok(matches!(
        record.policy_mode.as_str(),
        "repository" | "branch_restricted"
    ))
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

pub(super) async fn closed_world_allows_jit(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    intent_id: i64,
    binding_digest: &str,
) -> Result<bool, HostError> {
    if crate::journal::connection::has_unresolved_legacy_work(connection).await?
        || !records::set_is_exclusive_to_connection(connection, connection_id, scale_set_id).await?
    {
        return Ok(false);
    }
    if !created_set_matches(connection, connection_id, scale_set_id, binding_digest).await? {
        return Ok(false);
    }
    let Some(target) =
        records::launch_row_by_id(connection, connection_id, scale_set_id, intent_id).await?
    else {
        return Ok(false);
    };
    let Some(request_id) = target.request_id else {
        return Ok(false);
    };
    if target.state != "pending"
        || target.reservation_state == "released"
        || target.reservation_binding_digest != binding_digest
        || !target.acquire_attempted
        || !target.acquire_resolved
        || !target.acquired
        || target.jit_requested
        || !matches_intent(
            connection,
            connection_id,
            scale_set_id,
            request_id,
            intent_id,
            binding_digest,
        )
        .await?
    {
        return Ok(false);
    }
    if !records::assigned_backlog_is_closed(connection, connection_id, scale_set_id, binding_digest)
        .await?
    {
        return Ok(false);
    }
    Ok(true)
}

async fn require_current_set(
    connection: &turso::Connection,
    connection_id: &str,
    expected_set_id: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<Option<i64>>(0)
        .map_err(|_| HostError::Journal)?;
    if actual == Some(expected_set_id) {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

async fn require_created_set(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    binding_digest: &str,
) -> Result<(), HostError> {
    if records::created_set_count(connection, connection_id, scale_set_id, binding_digest).await? {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn require_fresh_source_target(
    launch: &LaunchRow,
    request_id: i64,
    binding_digest: &str,
) -> Result<(), HostError> {
    let expected_runner_name = format!("v{request_id}");
    if launch.id <= 0
        || launch.state != "pending"
        || launch.request_id != Some(request_id)
        || launch.runner_name.as_deref() != Some(expected_runner_name.as_str())
        || launch.acquire_attempted
        || launch.acquire_resolved
        || launch.acquired
        || launch.jit_requested
        || launch.reservation_state != "reserved"
        || launch.reservation_binding_digest != binding_digest
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn compare_replay(
    existing: SourceRow,
    launch: &LaunchRow,
    receipt: &AuthorizedSourceReceipt,
    binding_digest: &str,
    source_digest: &str,
    source_json: &str,
    policy_mode: &'static str,
) -> Result<SourceAuthorizationResult, HostError> {
    if existing.intent_id != launch.id
        || existing.message_id != receipt.message_id()
        || existing.policy_mode != policy_mode
        || existing.authority_model != AUTHORITY_MODEL
        || existing.binding_digest != binding_digest
        || existing.source_digest != source_digest
        || existing.source_json != source_json
    {
        return Err(HostError::Identity);
    }
    Ok(SourceAuthorizationResult::Replayed)
}

fn policy_mode(mode: SourcePolicyMode) -> &'static str {
    match mode {
        SourcePolicyMode::Repository => "repository",
        SourcePolicyMode::BranchRestricted => "branch_restricted",
    }
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
