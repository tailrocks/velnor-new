//! Durable challenge context and insert guards.

use crate::error::HostError;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;

pub(super) struct ChallengeContext {
    pub(super) connection_id: String,
    pub(super) reservation_id: i64,
    pub(super) intent_id: i64,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) binding_digest: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) generation: i64,
    pub(super) policy_digest: String,
}

mod records;
pub(super) use records::challenge_context;

pub(super) fn channel_matches_context(
    channel: &VerifiedProofChannelBinding,
    context: &ChallengeContext,
) -> bool {
    channel.reservation_id() == context.reservation_id
        && channel.operation_id() == context.intent_id
        && channel.connection_id() == context.connection_id
        && channel.scale_set_id() == context.scale_set_id
        && channel.witness_request_id() == context.witness_request_id
        && channel.engine_id() == context.engine_id
        && channel.docker_root_digest() == context.docker_root_digest
        && channel.runner_id() == context.runner_id
        && channel.runner_name() == context.runner_name
        && channel.runner_container_id() == context.runner_container_id
        && channel.dind_container_id() == context.dind_container_id
        && channel.instance_binding_digest() == context.instance_binding_digest
}

pub(super) async fn metadata_volume_matches(
    connection: &turso::Connection,
    reservation_id: i64,
    channel: &VerifiedProofChannelBinding,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT docker_name, docker_id, publication_policy_digest, create_state FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let name: String = row.get(0).map_err(|_| HostError::Journal)?;
    let object_id: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let publication_policy: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    let create_state: String = row.get(3).map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_none()
        && name == channel.metadata_volume_name()
        && object_id.as_deref() == Some(channel.metadata_volume_id())
        && publication_policy.as_deref().is_some_and(valid_digest)
        && create_state == "created")
}

pub(super) async fn proof_exists(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM connection_execution_proofs WHERE reservation_id = ?1)",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?
        != 0)
}

pub(super) async fn insert_requested_proof(
    connection: &turso::Connection,
    context: &ChallengeContext,
    challenge: &str,
    channel_digest: &str,
    channel_json: &str,
    challenge_json: &str,
    challenge_digest: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO connection_execution_proofs (intent_id, reservation_id, connection_id, scale_set_id, witness_request_id, binding_digest, generation, policy_digest, engine_id, docker_root_digest, runner_id, runner_name, runner_container_id, dind_container_id, instance_binding_digest, challenge, channel_digest, channel_json, challenge_state, challenge_json, challenge_digest, proof_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'not_requested', ?19, ?20, 'pending')",
            turso::params![
                context.intent_id,
                context.reservation_id,
                context.connection_id.as_str(),
                context.scale_set_id,
                context.witness_request_id,
                context.binding_digest.as_str(),
                context.generation,
                context.policy_digest.as_str(),
                context.engine_id.as_str(),
                context.docker_root_digest.as_str(),
                context.runner_id,
                context.runner_name.as_str(),
                context.runner_container_id.as_str(),
                context.dind_container_id.as_str(),
                context.instance_binding_digest.as_str(),
                challenge,
                channel_digest,
                channel_json,
                challenge_json,
                challenge_digest,
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let changed = connection
        .execute(
            "UPDATE connection_execution_proofs SET challenge_state = 'requested' WHERE intent_id = ?1 AND reservation_id = ?2 AND challenge_state = 'not_requested' AND challenge = ?3",
            (context.intent_id, context.reservation_id, challenge),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::challenge_context;
    use crate::error::HostError;

    #[tokio::test]
    async fn challenge_context_rejects_publication_runner_mismatch() -> Result<(), HostError> {
        let fixture =
            crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
        let reservation_id = fixture.initialized.reservation_id;
        let path = fixture.database_path.to_str().ok_or(HostError::Journal)?;
        let database = turso::Builder::new_local(path)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        let connection = database.connect().map_err(|_| HostError::Journal)?;
        assert!(
            challenge_context(
                &connection,
                reservation_id,
                fixture.scope.connection_id().as_str(),
            )
            .await?
            .is_some(),
            "The test fixture must permit the source check."
        );
        connection
            .execute("DROP TRIGGER provisioning_publication_no_update", ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "UPDATE capacity_reservation_provisioning_publications SET runner_id = runner_id + 1 WHERE reservation_id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;

        assert!(
            challenge_context(
                &connection,
                reservation_id,
                fixture.scope.connection_id().as_str(),
            )
            .await?
            .is_none()
        );
        Ok(())
    }
}
