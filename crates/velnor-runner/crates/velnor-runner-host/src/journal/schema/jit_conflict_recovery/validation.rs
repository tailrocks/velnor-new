//! Strict schema and identity validation for persisted JIT conflict receipts.

use crate::error::HostError;

use super::ddl;

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for (kind, name, expected) in [
        (
            "table",
            "physical_attempt_jit_conflict_receipts",
            ddl::TABLE,
        ),
        (
            "index",
            "physical_attempt_jit_conflict_receipts_witness",
            ddl::WITNESS_INDEX,
        ),
        (
            "trigger",
            "physical_attempt_jit_conflict_receipt_insert_guard",
            ddl::INSERT_GUARD,
        ),
        (
            "trigger",
            "physical_attempt_jit_conflict_receipt_no_update",
            ddl::NO_UPDATE,
        ),
        (
            "trigger",
            "physical_attempt_jit_conflict_receipt_no_delete",
            ddl::NO_DELETE,
        ),
    ] {
        validate_object(conn, kind, name, expected).await?;
    }
    for (suffix, action) in [
        ("insert", "after insert"),
        ("update", "after update"),
        ("delete", "after delete"),
    ] {
        let name = format!("physical_attempt_jit_conflict_receipts_revision_{suffix}");
        let expected = format!(
            "CREATE TRIGGER {name} {action} ON physical_attempt_jit_conflict_receipts BEGIN UPDATE journal_meta SET revision=revision+1 WHERE singleton=1; END"
        );
        validate_object(conn, "trigger", &name, &expected).await?;
    }
    validate_rows(conn).await
}

async fn validate_object(
    conn: &turso::Connection,
    kind: &str,
    name: &str,
    expected: &str,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || super::super::sql_normalization::normalize(&actual)
            != super::super::sql_normalization::normalize(expected)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_jit_conflict_receipts receipt LEFT JOIN physical_attempts attempt ON attempt.lineage_id=receipt.lineage_id AND attempt.generation=receipt.generation AND attempt.reservation_id=receipt.reservation_id AND attempt.intent_id=receipt.intent_id LEFT JOIN connection_request_authorizations auth ON auth.connection_id=receipt.connection_id AND auth.scale_set_id=receipt.scale_set_id AND auth.request_id=receipt.witness_request_id LEFT JOIN connection_job_authority_policies policy ON policy.connection_id=receipt.connection_id AND policy.generation=receipt.authority_generation AND policy.policy_digest=receipt.authority_digest LEFT JOIN connection_runner_jits jit ON jit.intent_id=receipt.intent_id AND jit.connection_id=receipt.connection_id WHERE attempt.lineage_id IS NULL OR attempt.connection_id!=receipt.connection_id OR attempt.scale_set_id!=receipt.scale_set_id OR attempt.witness_request_id!=receipt.witness_request_id OR attempt.binding_digest!=receipt.binding_digest OR attempt.source_digest!=receipt.source_digest OR (attempt.acquire_response_digest IS NOT NULL AND attempt.acquire_response_digest!=receipt.acquire_response_digest) OR attempt.engine_id!=receipt.engine_id OR attempt.docker_root_digest!=receipt.docker_root_digest OR auth.connection_id IS NULL OR auth.binding_digest!=receipt.binding_digest OR auth.source_digest!=receipt.source_digest OR auth.acquire_response_digest IS NOT receipt.acquire_response_digest OR auth.acquire_state!='acquired' OR policy.connection_id IS NULL OR policy.binding_digest!=receipt.binding_digest OR jit.intent_id IS NULL OR jit.scale_set_id!=receipt.scale_set_id OR jit.witness_request_id!=receipt.witness_request_id OR jit.requested_name!=receipt.requested_name OR receipt.response_status!=409",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}
