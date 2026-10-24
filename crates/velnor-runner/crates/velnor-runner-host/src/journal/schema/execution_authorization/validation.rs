//! Reopen checks for immutable authority snapshots and effect receipts.

mod channels;
mod decisions;
mod policies;
mod proofs;

use crate::error::HostError;

pub(super) async fn validate_rows(connection: &turso::Connection) -> Result<(), HostError> {
    policies::validate(connection).await?;
    proofs::validate(connection).await?;
    decisions::validate(connection).await?;
    validate_relations(connection).await
}

async fn validate_relations(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = count(
        connection,
        "SELECT (SELECT COUNT(*) FROM connection_active_job_authority AS active LEFT JOIN connection_job_authority_policies AS policy ON policy.connection_id = active.connection_id AND policy.generation = active.generation AND policy.policy_digest = active.policy_digest WHERE policy.connection_id IS NULL) + (SELECT COUNT(*) FROM connection_execution_proofs AS proof WHERE (proof.proof_state = 'pending' AND EXISTS (SELECT 1 FROM connection_execution_decisions AS decision WHERE decision.intent_id = proof.intent_id)) OR (proof.proof_state IN ('approved', 'denied') AND proof.actual_request_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM connection_execution_decisions AS decision WHERE decision.intent_id = proof.intent_id)) OR (proof.actual_request_id IS NULL AND EXISTS (SELECT 1 FROM connection_execution_decisions AS decision WHERE decision.intent_id = proof.intent_id)))",
    )
    .await?;
    if invalid == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn count(connection: &turso::Connection, query: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(query, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

pub(super) fn digest(value: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

pub(super) fn canonical_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, HostError> {
    serde_json::to_vec(value).map_err(|_| HostError::Journal)
}
