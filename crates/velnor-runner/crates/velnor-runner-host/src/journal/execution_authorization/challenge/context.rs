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

struct ReservationContext {
    intent_id: i64,
    binding_digest: String,
    engine_id: String,
    docker_root_digest: String,
    scale_set_id: i64,
    witness_request_id: i64,
    runner_id: i64,
    runner_name: String,
}

struct WitnessQuery<'a> {
    reservation_id: i64,
    connection_id: &'a str,
    intent_id: i64,
    binding_digest: &'a str,
    scale_set_id: i64,
    intent_runner_name: &'a str,
    github_runner_id: &'a str,
}

struct RuntimeContext {
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    generation: i64,
    policy_digest: String,
}

pub(super) async fn challenge_context(
    connection: &turso::Connection,
    reservation_id: i64,
    connection_id: &str,
) -> Result<Option<ChallengeContext>, HostError> {
    // Keep this lookup as bounded reads. Turso's expression translator recurses
    // deeply for the full 32-term authorization predicate as one WHERE clause.
    // The caller holds the journal transaction, so the staged reads observe one
    // consistent state while each guard remains explicit and fail-closed.
    let Some(reservation) = reservation_context(connection, reservation_id, connection_id).await?
    else {
        return Ok(None);
    };
    let Some(runtime) =
        runtime_context(connection, reservation_id, connection_id, &reservation).await?
    else {
        return Ok(None);
    };
    Ok(Some(ChallengeContext {
        connection_id: connection_id.to_owned(),
        reservation_id,
        intent_id: reservation.intent_id,
        scale_set_id: reservation.scale_set_id,
        witness_request_id: reservation.witness_request_id,
        binding_digest: reservation.binding_digest,
        engine_id: reservation.engine_id,
        docker_root_digest: reservation.docker_root_digest,
        runner_id: reservation.runner_id,
        runner_name: reservation.runner_name,
        runner_container_id: runtime.runner_container_id,
        dind_container_id: runtime.dind_container_id,
        instance_binding_digest: runtime.instance_binding_digest,
        generation: runtime.generation,
        policy_digest: runtime.policy_digest,
    }))
}

async fn reservation_context(
    connection: &turso::Connection,
    reservation_id: i64,
    connection_id: &str,
) -> Result<Option<ReservationContext>, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id, binding_digest, engine_id, docker_root_digest, state, remote_cleanup_proven, local_cleanup_proven FROM capacity_reservations WHERE id = ?1 AND connection_id = ?2",
            (reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let intent_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let binding_digest: String = row.get(1).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(2).map_err(|_| HostError::Journal)?;
    let docker_root_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    if row.get::<String>(4).map_err(|_| HostError::Journal)? != "reserved"
        || row.get::<i64>(5).map_err(|_| HostError::Journal)? != 0
        || row.get::<i64>(6).map_err(|_| HostError::Journal)? != 0
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Ok(None);
    }
    let Some((scale_set_id, intent_runner_name, github_runner_id)) =
        intent_context(connection, intent_id, connection_id).await?
    else {
        return Ok(None);
    };
    let query = WitnessQuery {
        reservation_id,
        connection_id,
        intent_id,
        binding_digest: &binding_digest,
        scale_set_id,
        intent_runner_name: &intent_runner_name,
        github_runner_id: &github_runner_id,
    };
    let Some((witness_request_id, runner_id, runner_name)) =
        witness_jit_context(connection, &query).await?
    else {
        return Ok(None);
    };
    Ok(Some(ReservationContext {
        intent_id,
        binding_digest,
        engine_id,
        docker_root_digest,
        scale_set_id,
        witness_request_id,
        runner_id,
        runner_name,
    }))
}

async fn intent_context(
    connection: &turso::Connection,
    intent_id: i64,
    connection_id: &str,
) -> Result<Option<(i64, String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT kind, scale_set_id, state, cleanup_proven, runner_completed, runner_request_id, runner_name, github_runner_id FROM connection_intents WHERE id = ?1 AND connection_id = ?2",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let valid = row.get::<String>(0).map_err(|_| HostError::Journal)? == "launch"
        && row.get::<String>(2).map_err(|_| HostError::Journal)? == "pending"
        && row.get::<i64>(3).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(4).map_err(|_| HostError::Journal)? == 0
        && row
            .get::<Option<i64>>(5)
            .map_err(|_| HostError::Journal)?
            .is_none()
        && rows.next().await.map_err(|_| HostError::Journal)?.is_none();
    if !valid {
        return Ok(None);
    }
    let (Some(scale_set_id), Some(runner_name), Some(github_runner_id)) = (
        row.get::<Option<i64>>(1).map_err(|_| HostError::Journal)?,
        row.get::<Option<String>>(6)
            .map_err(|_| HostError::Journal)?,
        row.get::<Option<String>>(7)
            .map_err(|_| HostError::Journal)?,
    ) else {
        return Ok(None);
    };
    Ok(Some((scale_set_id, runner_name, github_runner_id)))
}

async fn witness_jit_context(
    connection: &turso::Connection,
    query: &WitnessQuery<'_>,
) -> Result<Option<(i64, i64, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id, request_id, binding_digest FROM connection_request_reservations WHERE reservation_id = ?1 AND connection_id = ?2",
            (query.reservation_id, query.connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let scale_set: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let witness_request_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || scale_set != query.scale_set_id
        || row.get::<String>(2).map_err(|_| HostError::Journal)? != query.binding_digest
    {
        return Ok(None);
    }

    let mut rows = connection
        .query(
            "SELECT runner_id, runner_name, scale_set_id, witness_request_id, state FROM connection_runner_jits WHERE intent_id = ?1 AND connection_id = ?2",
            (query.intent_id, query.connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let (Some(runner_id), Some(runner_name)) = (
        row.get::<Option<i64>>(0).map_err(|_| HostError::Journal)?,
        row.get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?,
    ) else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || runner_name != query.intent_runner_name
        || query.github_runner_id != runner_id.to_string()
        || row.get::<Option<i64>>(2).map_err(|_| HostError::Journal)? != Some(query.scale_set_id)
        || row.get::<Option<i64>>(3).map_err(|_| HostError::Journal)? != Some(witness_request_id)
        || row.get::<String>(4).map_err(|_| HostError::Journal)? != "bound"
    {
        return Ok(None);
    }
    Ok(Some((witness_request_id, runner_id, runner_name)))
}

async fn runtime_context(
    connection: &turso::Connection,
    reservation_id: i64,
    connection_id: &str,
    reservation: &ReservationContext,
) -> Result<Option<RuntimeContext>, HostError> {
    if !await_request_is_acquired(
        connection,
        connection_id,
        reservation.scale_set_id,
        reservation.witness_request_id,
    )
    .await?
        || !await_provisioning_matches(connection, reservation_id, reservation).await?
    {
        return Ok(None);
    }
    let Some((runner_container_id, dind_container_id, instance_binding_digest)) =
        provisioning_state(connection, reservation_id).await?
    else {
        return Ok(None);
    };
    if !publication_matches(connection, reservation_id, reservation).await? {
        return Ok(None);
    }
    let Some((generation, policy_digest)) = active_policy(connection, connection_id).await? else {
        return Ok(None);
    };
    Ok(Some(RuntimeContext {
        runner_container_id,
        dind_container_id,
        instance_binding_digest,
        generation,
        policy_digest,
    }))
}

async fn publication_matches(
    connection: &turso::Connection,
    reservation_id: i64,
    reservation: &ReservationContext,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_id, runner_name, runner_scale_set_id FROM capacity_reservation_provisioning_publications WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let runner_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let runner_name: String = row.get(1).map_err(|_| HostError::Journal)?;
    let scale_set_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_none()
        && runner_id == reservation.runner_id
        && runner_name == reservation.runner_name
        && scale_set_id == reservation.scale_set_id)
}

async fn await_request_is_acquired(
    connection: &turso::Connection,
    connection_id: &str,
    scale_set_id: i64,
    witness_request_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT acquire_state FROM connection_request_authorizations WHERE connection_id = ?1 AND scale_set_id = ?2 AND request_id = ?3",
            (connection_id, scale_set_id, witness_request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_none()
        && row.get::<String>(0).map_err(|_| HostError::Journal)? == "acquired")
}

async fn await_provisioning_matches(
    connection: &turso::Connection,
    reservation_id: i64,
    reservation: &ReservationContext,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT engine_id, docker_root_digest FROM capacity_reservation_provisioning WHERE reservation_id = ?1 AND operation_id = ?2",
            (reservation_id, reservation.intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_none()
        && row.get::<String>(0).map_err(|_| HostError::Journal)? == reservation.engine_id
        && row.get::<String>(1).map_err(|_| HostError::Journal)? == reservation.docker_root_digest)
}

async fn provisioning_state(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<(String, String, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT resources_bound, bootstrap_start_state, jit_state, publication_state, delivery_state, storage_cleanup_proven, runner_container_id, dind_container_id, instance_binding_digest FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if row.get::<i64>(0).map_err(|_| HostError::Journal)? != 1
        || row.get::<String>(1).map_err(|_| HostError::Journal)? != "started"
        || row.get::<String>(2).map_err(|_| HostError::Journal)? != "bound"
        || row.get::<String>(3).map_err(|_| HostError::Journal)? != "published"
        || row.get::<String>(4).map_err(|_| HostError::Journal)? != "not_requested"
        || row.get::<i64>(5).map_err(|_| HostError::Journal)? != 0
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Ok(None);
    }
    let (Some(runner), Some(dind), Some(binding)) = (
        row.get::<Option<String>>(6)
            .map_err(|_| HostError::Journal)?,
        row.get::<Option<String>>(7)
            .map_err(|_| HostError::Journal)?,
        row.get::<Option<String>>(8)
            .map_err(|_| HostError::Journal)?,
    ) else {
        return Ok(None);
    };
    Ok(Some((runner, dind, binding)))
}

async fn active_policy(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<(i64, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT generation, policy_digest FROM connection_active_job_authority WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    )))
}

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
            crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
        let path = fixture.database_path.to_str().ok_or(HostError::Journal)?;
        let database = turso::Builder::new_local(path)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        let connection = database.connect().map_err(|_| HostError::Journal)?;
        connection
            .execute("DROP TRIGGER provisioning_publication_no_update", ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "UPDATE capacity_reservation_provisioning_publications SET runner_id = runner_id + 1 WHERE reservation_id = ?1",
                [fixture.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;

        assert!(
            challenge_context(
                &connection,
                fixture.reservation_id,
                fixture.scope.connection_id().as_str(),
            )
            .await?
            .is_none()
        );
        Ok(())
    }
}
