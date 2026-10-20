//! Read durable challenge authorization records.

use crate::error::HostError;

use super::ChallengeContext;

pub(super) struct ReservationContext {
    pub(super) intent_id: i64,
    pub(super) binding_digest: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
}

pub(super) struct WitnessQuery<'a> {
    pub(super) reservation_id: i64,
    pub(super) connection_id: &'a str,
    intent_id: i64,
    binding_digest: &'a str,
    scale_set_id: i64,
    pub(super) intent_runner_name: &'a str,
    pub(super) github_runner_id: &'a str,
}

pub(super) struct RuntimeContext {
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) generation: i64,
    pub(super) policy_digest: String,
}

pub(in crate::journal::execution_authorization::challenge) async fn challenge_context(
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

pub(super) async fn reservation_context(
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

pub(super) async fn intent_context(
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

pub(super) async fn witness_jit_context(
    connection: &turso::Connection,
    query: &WitnessQuery<'_>,
) -> Result<Option<(i64, i64, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT attempt.scale_set_id, attempt.witness_request_id, attempt.binding_digest FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id WHERE current.reservation_id = ?1 AND attempt.connection_id = ?2",
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

pub(super) async fn runtime_context(
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

pub(super) async fn publication_matches(
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

pub(super) async fn await_request_is_acquired(
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

pub(super) async fn await_provisioning_matches(
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

pub(super) async fn provisioning_state(
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

pub(super) async fn active_policy(
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
