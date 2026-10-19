use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use velnor_runner_github::RunnerReference;

use super::super::{hold_context, hold_is_claimable};
use super::{JobFactsPublicationClaim, ListenerDeliveryClaim};

pub(super) async fn bound_state(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<super::BoundState>, HostError> {
    let mut rows = connection
        .query(
            "SELECT bootstrap_start_state, resources_bound, runner_container_id, dind_container_id, instance_binding_digest, jit_state, publication_state, delivery_state, storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(super::BoundState {
        bootstrap_state: row.get(0).map_err(|_| HostError::Journal)?,
        resources_bound: row.get::<i64>(1).map_err(|_| HostError::Journal)? == 1,
        runner_container_id: row.get(2).map_err(|_| HostError::Journal)?,
        dind_container_id: row.get(3).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(4).map_err(|_| HostError::Journal)?,
        jit_state: row.get(5).map_err(|_| HostError::Journal)?,
        publication_state: row.get(6).map_err(|_| HostError::Journal)?,
        delivery_state: row.get(7).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get::<i64>(8).map_err(|_| HostError::Journal)? == 1,
    }))
}

pub(super) async fn jit_runner(
    connection: &turso::Connection,
    connection_id: &str,
    operation_id: i64,
) -> Result<RunnerReference, HostError> {
    let mut rows = connection
        .query(
            "SELECT j.runner_id, j.runner_name, j.scale_set_id FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id WHERE j.intent_id = ?1 AND j.connection_id = ?2 AND j.state = 'bound' AND j.runner_id > 0 AND j.runner_name IS NOT NULL AND i.state = 'pending' AND i.cleanup_proven = 0",
            (operation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        name: row.get(1).map_err(|_| HostError::Journal)?,
        runner_scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
    };
    if runner.id <= 0 || runner.runner_scale_set_id <= 0 || runner.name.is_empty() {
        return Err(HostError::Journal);
    }
    Ok(runner)
}

pub(super) async fn validate_claim_state(
    connection: &turso::Connection,
    claim: &JobFactsPublicationClaim,
    publication_state: &str,
    jit_state: &str,
) -> Result<(), HostError> {
    let context = hold_context(connection, &claim.connection_id, claim.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if context.operation_id != claim.operation_id
        || context.engine_id != claim.engine_id
        || context.docker_root_digest != claim.docker_root_digest
        || !hold_is_claimable(&context)
    {
        return Err(HostError::Journal);
    }
    let state = bound_state(connection, claim.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if state.bootstrap_state != "started"
        || !state.resources_bound
        || state.instance_binding_digest != claim.instance_binding_digest
        || state.jit_state != jit_state
        || state.publication_state != publication_state
        || state.cleanup_proven
        || state.runner_container_id != claim.runner_container_id
        || state.dind_container_id != claim.dind_container_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn validate_delivery_state(
    connection: &turso::Connection,
    claim: &ListenerDeliveryClaim,
) -> Result<(), HostError> {
    let context = hold_context(connection, &claim.connection_id, claim.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if context.operation_id != claim.operation_id
        || context.engine_id != claim.engine_id
        || context.docker_root_digest != claim.docker_root_digest
        || !hold_is_claimable(&context)
    {
        return Err(HostError::Journal);
    }
    let state = bound_state(connection, claim.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if state.bootstrap_state != "started"
        || !state.resources_bound
        || state.instance_binding_digest != claim.instance_binding_digest
        || state.jit_state != "bound"
        || state.publication_state != "published"
        || state.delivery_state != "requested"
        || state.cleanup_proven
        || state.runner_container_id != claim.runner_container_id
    {
        return Err(HostError::Journal);
    }
    if !same_runner(
        &jit_runner(connection, &claim.connection_id, claim.operation_id).await?,
        &claim.runner,
    ) {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn validate_publication_claim(
    journal: &ConnectionJournal,
    claim: &JobFactsPublicationClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || claim.runner.id <= 0
        || claim.runner.runner_scale_set_id <= 0
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

pub(super) fn validate_delivery_claim(
    journal: &ConnectionJournal,
    claim: &ListenerDeliveryClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || claim.runner.id <= 0
        || claim.runner.runner_scale_set_id <= 0
        || claim.runner_container_id.is_empty()
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

pub(super) fn same_runner(left: &RunnerReference, right: &RunnerReference) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.runner_scale_set_id == right.runner_scale_set_id
}

pub(super) fn packed_device(major: u32, minor: u32) -> Result<i64, HostError> {
    let packed = (u64::from(major) << 32) | u64::from(minor);
    i64::try_from(packed).map_err(|_| HostError::Journal)
}
