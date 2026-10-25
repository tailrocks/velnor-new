use crate::error::HostError;

use super::HoldContext;

pub(super) async fn initial_eligible(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    context: &HoldContext,
) -> Result<bool, HostError> {
    if context.reservation_state != "uncertain"
        || context.intent_state != "uncertain"
        || context.cleanup_proven
        || context.topology_version != 2
    {
        return Ok(false);
    }
    let mut rows = connection.query(
        "SELECT state.bootstrap_start_state, state.resources_bound, state.jit_state, state.publication_state, state.delivery_state, state.storage_cleanup_proven, COUNT(response.role), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id AND bootstrap.create_state NOT IN ('unclaimed', 'created', 'uncertain')), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id AND bootstrap.start_state != 'not_requested'), (SELECT COUNT(*) FROM capacity_reservation_pre_jit_create_responses AS receipt JOIN capacity_reservation_bootstrap_containers AS bootstrap ON bootstrap.reservation_id = receipt.reservation_id AND bootstrap.role = receipt.role WHERE receipt.reservation_id = state.reservation_id AND receipt.connection_id = ?2 AND receipt.operation_id = ?3 AND receipt.engine_id = ?4 AND receipt.docker_root_digest = ?5 AND bootstrap.create_state = 'uncertain' AND bootstrap.container_id IS NULL AND bootstrap.start_state = 'not_requested'), (SELECT COUNT(*) FROM connection_runner_jits AS jit WHERE jit.intent_id = ?3 AND jit.connection_id = ?2), (SELECT COUNT(*) FROM connection_execution_proofs AS proof WHERE proof.intent_id = ?3 AND proof.connection_id = ?2), (SELECT COUNT(*) FROM connection_execution_decisions AS decision WHERE decision.intent_id = ?3 AND decision.connection_id = ?2), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id), (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS object WHERE object.reservation_id = state.reservation_id AND object.create_state = 'created'), (SELECT COUNT(*) FROM capacity_reservation_networks AS network WHERE network.reservation_id = state.reservation_id AND network.create_state = 'created' AND network.remove_state IN ('not_requested', 'requested', 'removed', 'uncertain')), (SELECT COUNT(*) FROM capacity_reservation_storage_initialization AS init WHERE init.reservation_id = state.reservation_id AND init.create_state = 'created' AND init.start_state = 'started' AND init.result_state = 'confirmed' AND init.remove_state = 'not_requested' AND init.abort_state = 'not_requested'), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id AND bootstrap.create_state = 'uncertain' AND bootstrap.container_id IS NULL AND bootstrap.start_state = 'not_requested'), ((SELECT COUNT(*) FROM capacity_reservation_pre_jit_create_specs AS spec WHERE spec.reservation_id = state.reservation_id AND spec.connection_id = ?2 AND spec.operation_id = ?3 AND spec.engine_id = ?4 AND spec.docker_root_digest = ?5) - (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id AND bootstrap.create_state != 'unclaimed')) FROM capacity_reservation_provisioning_state AS state LEFT JOIN capacity_reservation_pre_jit_create_responses AS response ON response.reservation_id = state.reservation_id AND response.connection_id = ?2 AND response.operation_id = ?3 AND response.engine_id = ?4 AND response.docker_root_digest = ?5 WHERE state.reservation_id = ?1 GROUP BY state.reservation_id",
        (reservation_id, connection_id, context.operation_id, context.engine_id.as_str(), context.docker_root_digest.as_str()),
    ).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let okay = row.get::<String>(0).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<i64>(1).map_err(|_| HostError::Journal)? == 0
        && row.get::<String>(2).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<String>(3).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<String>(4).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<i64>(5).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(6).map_err(|_| HostError::Journal)? > 0;
    let okay = okay
        && row.get::<i64>(7).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(8).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(9).map_err(|_| HostError::Journal)? > 0
        && row.get::<i64>(10).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(11).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(12).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(13).map_err(|_| HostError::Journal)? == 2
        && row.get::<i64>(14).map_err(|_| HostError::Journal)? == 5
        && row.get::<i64>(15).map_err(|_| HostError::Journal)? == 1
        && row.get::<i64>(16).map_err(|_| HostError::Journal)? == 1
        && row.get::<i64>(6).map_err(|_| HostError::Journal)?
            == row.get::<i64>(9).map_err(|_| HostError::Journal)?
        && row.get::<i64>(17).map_err(|_| HostError::Journal)?
            == row.get::<i64>(9).map_err(|_| HostError::Journal)?
        && row.get::<i64>(18).map_err(|_| HostError::Journal)? == 0;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(okay)
}

pub(super) async fn claim_eligible(
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    context: &HoldContext,
) -> Result<bool, HostError> {
    if context.reservation_state != "uncertain"
        || context.intent_state != "uncertain"
        || context.cleanup_proven
        || context.topology_version != 2
    {
        return Ok(false);
    }
    let mut rows = connection
        .query(
            "SELECT state.bootstrap_start_state, state.resources_bound, state.jit_state, state.publication_state, state.delivery_state, state.storage_cleanup_proven, (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id), (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id AND bootstrap.start_state != 'not_requested'), (SELECT COUNT(*) FROM capacity_reservation_pre_jit_create_responses AS response WHERE response.reservation_id = state.reservation_id AND response.connection_id = ?2 AND response.operation_id = ?3 AND response.engine_id = ?4 AND response.docker_root_digest = ?5), (SELECT COUNT(*) FROM connection_runner_jits AS jit WHERE jit.intent_id = ?3 AND jit.connection_id = ?2), (SELECT COUNT(*) FROM connection_execution_proofs AS proof WHERE proof.intent_id = ?3 AND proof.connection_id = ?2), (SELECT COUNT(*) FROM connection_execution_decisions AS decision WHERE decision.intent_id = ?3 AND decision.connection_id = ?2), ((SELECT COUNT(*) FROM capacity_reservation_pre_jit_create_specs AS spec WHERE spec.reservation_id = state.reservation_id AND spec.connection_id = ?2 AND spec.operation_id = ?3 AND spec.engine_id = ?4 AND spec.docker_root_digest = ?5) - (SELECT COUNT(*) FROM capacity_reservation_bootstrap_containers AS bootstrap WHERE bootstrap.reservation_id = state.reservation_id AND bootstrap.create_state != 'unclaimed')) FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = ?1",
            (
                reservation_id,
                connection_id,
                context.operation_id,
                context.engine_id.as_str(),
                context.docker_root_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let eligible = row.get::<String>(0).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<i64>(1).map_err(|_| HostError::Journal)? == 0
        && row.get::<String>(2).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<String>(3).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<String>(4).map_err(|_| HostError::Journal)? == "not_requested"
        && row.get::<i64>(5).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(6).map_err(|_| HostError::Journal)? == 2
        && row.get::<i64>(7).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(8).map_err(|_| HostError::Journal)? > 0
        && row.get::<i64>(9).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(10).map_err(|_| HostError::Journal)? == 0
        && row.get::<i64>(11).map_err(|_| HostError::Journal)? == 0;
    let eligible = eligible && row.get::<i64>(12).map_err(|_| HostError::Journal)? == 0;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(eligible)
}
