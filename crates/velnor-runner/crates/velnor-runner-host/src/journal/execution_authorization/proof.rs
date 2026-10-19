//! Store one verified source request before checking the native Started event.

use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::execution_authorization::records::{
    ExecutionProofProgress, ProofChannelRecord, digest,
};
use crate::launch::execution_authorization::{AuthorizedExecutionSource, ExecutionProofChallenge};

pub(super) async fn record_source(
    scope: &ConnectionJournal,
    source: &AuthorizedExecutionSource,
) -> Result<ExecutionProofProgress, HostError> {
    source.validate()?;
    let request = source.request();
    let source_bytes = source.canonical_json()?;
    let source_json = String::from_utf8(source_bytes.clone()).map_err(|_| HostError::Journal)?;
    let source_digest = digest(&source_bytes);
    let connection_id = scope.connection_id().as_str().to_owned();
    if request.reservation_id() <= 0 || request.intent_id() <= 0 || request.challenge().len() != 64
    {
        return Err(HostError::Identity);
    }

    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let stored = load_proof(
            connection,
            request.intent_id(),
            request.reservation_id(),
            &connection_id,
        )
        .await?;
        let Some(stored) = stored else {
            return Err(HostError::Identity);
        };
        if stored.challenge != request.challenge()
            || stored.challenge_state != "published"
            || stored.proof_state != "pending"
            || !source.matches_challenge(&stored.challenge_document)
        {
            return Err(HostError::Identity);
        }
        if let Some(existing) = stored.source_digest.as_deref() {
            if existing == source_digest && stored.source_json.as_deref() == Some(&source_json) {
                return Ok(ExecutionProofProgress::SourceRecorded);
            }
            return Err(HostError::Journal);
        }
        if stored.source_json.is_some() {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE connection_execution_proofs SET source_digest = ?1, source_json = ?2 WHERE intent_id = ?3 AND reservation_id = ?4 AND connection_id = ?5 AND challenge = ?6 AND challenge_state = 'published' AND proof_state = 'pending' AND source_digest IS NULL AND source_json IS NULL",
                turso::params![
                    source_digest,
                    source_json,
                    request.intent_id(),
                    request.reservation_id(),
                    connection_id.as_str(),
                    request.challenge(),
                ],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        Ok(ExecutionProofProgress::SourceRecorded)
    })
    .await
}

struct StoredProof {
    challenge: String,
    challenge_state: String,
    challenge_document: ExecutionProofChallenge,
    proof_state: String,
    source_digest: Option<String>,
    source_json: Option<String>,
}

async fn load_proof(
    connection: &turso::Connection,
    intent_id: i64,
    reservation_id: i64,
    connection_id: &str,
) -> Result<Option<StoredProof>, HostError> {
    let mut rows = connection
        .query(
            "SELECT challenge, challenge_state, challenge_json, challenge_digest, channel_digest, channel_json, proof_state, source_digest, source_json, binding_digest, generation, policy_digest, scale_set_id, witness_request_id, engine_id, docker_root_digest, runner_id, runner_name, runner_container_id, dind_container_id, instance_binding_digest FROM connection_execution_proofs WHERE intent_id = ?1 AND reservation_id = ?2 AND connection_id = ?3",
            (intent_id, reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    validate_stored_proof(&row, intent_id, reservation_id, connection_id).map(Some)
}

fn validate_stored_proof(
    row: &turso::Row,
    intent_id: i64,
    reservation_id: i64,
    connection_id: &str,
) -> Result<StoredProof, HostError> {
    let challenge: String = row.get(0).map_err(|_| HostError::Journal)?;
    let challenge_state: String = row.get(1).map_err(|_| HostError::Journal)?;
    let challenge_json: String = row.get(2).map_err(|_| HostError::Journal)?;
    let challenge_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    let channel_digest: String = row.get(4).map_err(|_| HostError::Journal)?;
    let channel_json: String = row.get(5).map_err(|_| HostError::Journal)?;
    let proof_state: String = row.get(6).map_err(|_| HostError::Journal)?;
    let source_digest: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
    let source_json: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let binding_digest: String = row.get(9).map_err(|_| HostError::Journal)?;
    let generation: i64 = row.get(10).map_err(|_| HostError::Journal)?;
    let policy_digest: String = row.get(11).map_err(|_| HostError::Journal)?;
    let scale_set_id: i64 = row.get(12).map_err(|_| HostError::Journal)?;
    let witness_request_id: i64 = row.get(13).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(14).map_err(|_| HostError::Journal)?;
    let docker_root_digest: String = row.get(15).map_err(|_| HostError::Journal)?;
    let runner_id: i64 = row.get(16).map_err(|_| HostError::Journal)?;
    let runner_name: String = row.get(17).map_err(|_| HostError::Journal)?;
    let runner_container_id: String = row.get(18).map_err(|_| HostError::Journal)?;
    let dind_container_id: String = row.get(19).map_err(|_| HostError::Journal)?;
    let instance_binding_digest: String = row.get(20).map_err(|_| HostError::Journal)?;
    let challenge_document: ExecutionProofChallenge =
        serde_json::from_str(&challenge_json).map_err(|_| HostError::Journal)?;
    let canonical_challenge = challenge_document.canonical_json()?;
    let channel = ProofChannelRecord::parse(&channel_json, &channel_digest)?;
    let generation_u64 = u64::try_from(generation).map_err(|_| HostError::Journal)?;
    if canonical_challenge != challenge_json.as_bytes()
        || digest(&canonical_challenge) != challenge_digest
        || !challenge_document.valid()
        || challenge_document.challenge() != challenge
        || challenge_document.reservation_id() != reservation_id
        || challenge_document.intent_id() != intent_id
        || challenge_document.witness_request_id() != witness_request_id
        || challenge_document.runner_id() != runner_id
        || challenge_document.runner_name() != runner_name
        || challenge_document.runner_scale_set_id() != scale_set_id
        || challenge_document.runner_container_id() != runner_container_id
        || challenge_document.dind_container_id() != dind_container_id
        || challenge_document.engine_id() != channel.engine_id()
        || challenge_document.docker_root_digest() != channel.docker_root_digest()
        || challenge_document.engine_id() != engine_id
        || challenge_document.docker_root_digest() != docker_root_digest
        || challenge_document.instance_binding_digest() != instance_binding_digest
        || challenge_document.policy_generation() != generation_u64
        || challenge_document.policy_digest() != policy_digest
        || challenge_document.channel_digest() != channel_digest
        || !channel.valid()
        || channel.reservation_id() != reservation_id
        || channel.operation_id() != intent_id
        || channel.connection_id() != connection_id
        || channel.scale_set_id() != scale_set_id
        || channel.witness_request_id() != witness_request_id
        || channel.engine_id() != engine_id
        || channel.docker_root_digest() != docker_root_digest
        || channel.runner_id() != runner_id
        || channel.runner_name() != runner_name
        || channel.runner_container_id() != runner_container_id
        || channel.dind_container_id() != dind_container_id
        || channel.instance_binding_digest() != instance_binding_digest
        || binding_digest.is_empty()
    {
        return Err(HostError::Journal);
    }
    Ok(StoredProof {
        challenge,
        challenge_state,
        challenge_document,
        proof_state,
        source_digest,
        source_json,
    })
}
