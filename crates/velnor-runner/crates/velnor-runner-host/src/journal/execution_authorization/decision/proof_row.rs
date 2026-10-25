//! Exact persisted challenge, channel, and source fields for one intent.

use crate::error::HostError;
use crate::journal::execution_authorization::records::{ProofChannelRecord, digest};
use crate::launch::execution_authorization::ExecutionProofChallenge;

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) struct ProofRow {
    pub(super) reservation_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) binding_digest: String,
    pub(super) generation: i64,
    pub(super) policy_digest: String,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
    pub(super) instance_binding_digest: String,
    pub(super) challenge: String,
    pub(super) challenge_state: String,
    pub(super) challenge_document: ExecutionProofChallenge,
    pub(super) source_digest: Option<String>,
    pub(super) source_json: Option<String>,
    pub(super) proof_state: String,
}

struct LoadedProofRow {
    reservation_id: i64,
    scale_set_id: i64,
    witness_request_id: i64,
    binding_digest: String,
    generation: i64,
    policy_digest: String,
    runner_id: i64,
    runner_name: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    challenge: String,
    challenge_state: String,
    challenge_json: String,
    challenge_digest: String,
    channel_digest: String,
    channel_json: String,
    source_digest: Option<String>,
    source_json: Option<String>,
    proof_state: String,
}

impl LoadedProofRow {
    fn from_turso(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
            scale_set_id: row.get(1).map_err(|_| HostError::Journal)?,
            witness_request_id: row.get(2).map_err(|_| HostError::Journal)?,
            binding_digest: row.get(3).map_err(|_| HostError::Journal)?,
            generation: row.get(4).map_err(|_| HostError::Journal)?,
            policy_digest: row.get(5).map_err(|_| HostError::Journal)?,
            runner_id: row.get(6).map_err(|_| HostError::Journal)?,
            runner_name: row.get(7).map_err(|_| HostError::Journal)?,
            runner_container_id: row.get(8).map_err(|_| HostError::Journal)?,
            dind_container_id: row.get(9).map_err(|_| HostError::Journal)?,
            instance_binding_digest: row.get(10).map_err(|_| HostError::Journal)?,
            challenge: row.get(11).map_err(|_| HostError::Journal)?,
            challenge_state: row.get(12).map_err(|_| HostError::Journal)?,
            challenge_json: row.get(13).map_err(|_| HostError::Journal)?,
            challenge_digest: row.get(14).map_err(|_| HostError::Journal)?,
            channel_digest: row.get(15).map_err(|_| HostError::Journal)?,
            channel_json: row.get(16).map_err(|_| HostError::Journal)?,
            source_digest: row.get(17).map_err(|_| HostError::Journal)?,
            source_json: row.get(18).map_err(|_| HostError::Journal)?,
            proof_state: row.get(19).map_err(|_| HostError::Journal)?,
        })
    }

    fn validate(self, operation_id: i64, connection_id: &str) -> Result<ProofRow, HostError> {
        let challenge_document: ExecutionProofChallenge =
            serde_json::from_str(&self.challenge_json).map_err(|_| HostError::Journal)?;
        let challenge_bytes = challenge_document.canonical_json()?;
        let channel = ProofChannelRecord::parse(&self.channel_json, &self.channel_digest)?;
        if !self.matches(
            &challenge_document,
            &challenge_bytes,
            &channel,
            operation_id,
            connection_id,
        )? {
            return Err(HostError::Journal);
        }
        Ok(ProofRow {
            reservation_id: self.reservation_id,
            scale_set_id: self.scale_set_id,
            witness_request_id: self.witness_request_id,
            binding_digest: self.binding_digest,
            generation: self.generation,
            policy_digest: self.policy_digest,
            runner_id: self.runner_id,
            runner_name: self.runner_name,
            instance_binding_digest: self.instance_binding_digest,
            challenge: self.challenge,
            challenge_state: self.challenge_state,
            challenge_document,
            source_digest: self.source_digest,
            source_json: self.source_json,
            proof_state: self.proof_state,
        })
    }

    fn matches(
        &self,
        challenge: &ExecutionProofChallenge,
        challenge_bytes: &[u8],
        channel: &ProofChannelRecord,
        operation_id: i64,
        connection_id: &str,
    ) -> Result<bool, HostError> {
        Ok(challenge_bytes == self.challenge_json.as_bytes()
            && digest(challenge_bytes) == self.challenge_digest
            && challenge.valid()
            && challenge.challenge() == self.challenge
            && challenge.reservation_id() == self.reservation_id
            && challenge.intent_id() == operation_id
            && challenge.witness_request_id() == self.witness_request_id
            && challenge.runner_id() == self.runner_id
            && challenge.runner_name() == self.runner_name
            && challenge.runner_scale_set_id() == self.scale_set_id
            && challenge.runner_container_id() == self.runner_container_id
            && challenge.dind_container_id() == self.dind_container_id
            && challenge.engine_id() == channel.engine_id()
            && challenge.docker_root_digest() == channel.docker_root_digest()
            && challenge.instance_binding_digest() == self.instance_binding_digest
            && challenge.policy_generation()
                == u64::try_from(self.generation).map_err(|_| HostError::Journal)?
            && challenge.policy_digest() == self.policy_digest
            && challenge.channel_digest() == self.channel_digest
            && channel.reservation_id() == self.reservation_id
            && channel.operation_id() == operation_id
            && channel.connection_id() == connection_id
            && channel.scale_set_id() == self.scale_set_id
            && channel.witness_request_id() == self.witness_request_id
            && channel.runner_id() == self.runner_id
            && channel.runner_name() == self.runner_name
            && channel.runner_container_id() == self.runner_container_id
            && channel.dind_container_id() == self.dind_container_id
            && channel.instance_binding_digest() == self.instance_binding_digest
            && valid_digest(&self.binding_digest)
            && valid_digest(&self.policy_digest))
    }
}

pub(super) async fn load_proof(
    connection: &turso::Connection,
    operation_id: i64,
    connection_id: &str,
) -> Result<Option<ProofRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT proof.reservation_id, proof.scale_set_id, proof.witness_request_id, proof.binding_digest, proof.generation, proof.policy_digest, proof.runner_id, proof.runner_name, proof.runner_container_id, proof.dind_container_id, proof.instance_binding_digest, proof.challenge, proof.challenge_state, proof.challenge_json, proof.challenge_digest, proof.channel_digest, proof.channel_json, proof.source_digest, proof.source_json, proof.proof_state FROM connection_execution_proofs AS proof JOIN capacity_reservations AS reservation ON reservation.id = proof.reservation_id AND reservation.intent_id = proof.intent_id AND reservation.connection_id = proof.connection_id AND reservation.binding_digest = proof.binding_digest AND reservation.engine_id = proof.engine_id AND reservation.docker_root_digest = proof.docker_root_digest JOIN connection_intents AS intent ON intent.id = proof.intent_id AND intent.connection_id = proof.connection_id AND intent.scale_set_id = proof.scale_set_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id AND provisioning.operation_id = intent.id AND provisioning.engine_id = proof.engine_id AND provisioning.docker_root_digest = proof.docker_root_digest JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id AND state.instance_binding_digest = proof.instance_binding_digest WHERE proof.intent_id = ?1 AND proof.connection_id = ?2 AND reservation.state = 'reserved' AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.cleanup_proven = 0 AND intent.runner_completed = 0",
            (operation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let proof = LoadedProofRow::from_turso(&row)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    proof.validate(operation_id, connection_id).map(Some)
}

#[cfg(test)]
mod tests {
    use super::load_proof;
    use crate::error::HostError;
    use crate::journal::request_lifecycle::tests::{
        NativePreChallengeFixture, native_pre_challenge_fixture, publish_native_fixture_challenge,
    };

    #[tokio::test]
    async fn proof_loader_rejects_container_identity_mismatches() -> Result<(), HostError> {
        for field in ["runner_container_id", "dind_container_id"] {
            let fixture = native_pre_challenge_fixture().await?;
            publish_native_fixture_challenge(&fixture).await?;
            let reservation_id = fixture.initialized.reservation_id;
            let operation_id = fixture.initialized.intent_id;
            let connection_id = fixture.scope.connection_id().as_str().to_owned();
            let connection = fixture.journal.connection().await?;
            let original = container_id(&connection, field, reservation_id).await?;
            let replacement = different_container_id(&original)?;
            let guard = runtime_guard(&connection).await?;
            connection
                .execute(
                    "DROP TRIGGER execution_proof_identity_runtime_transition_guard",
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            connection
                .execute(
                    &format!("UPDATE connection_execution_proofs SET {field} = ?1 WHERE reservation_id = ?2"),
                    (replacement.as_str(), reservation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if load_proof(&connection, operation_id, &connection_id)
                .await
                .is_ok()
            {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    &format!("UPDATE connection_execution_proofs SET {field} = ?1 WHERE reservation_id = ?2"),
                    (original.as_str(), reservation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            connection
                .execute(&guard, ())
                .await
                .map_err(|_| HostError::Journal)?;
            drop(connection);
            let NativePreChallengeFixture { journal, .. } = fixture;
            drop(journal);
        }
        Ok(())
    }

    async fn container_id(
        connection: &turso::Connection,
        field: &str,
        reservation_id: i64,
    ) -> Result<String, HostError> {
        let mut rows = connection
            .query(
                &format!(
                    "SELECT {field} FROM connection_execution_proofs WHERE reservation_id = ?1"
                ),
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)
    }

    async fn runtime_guard(connection: &turso::Connection) -> Result<String, HostError> {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'execution_proof_identity_runtime_transition_guard'",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)
    }

    fn different_container_id(value: &str) -> Result<String, HostError> {
        let mut bytes = value.as_bytes().to_vec();
        let first = bytes.first_mut().ok_or(HostError::Journal)?;
        *first = if *first == b'0' { b'1' } else { b'0' };
        String::from_utf8(bytes).map_err(|_| HostError::Journal)
    }
}
