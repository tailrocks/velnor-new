use crate::error::HostError;
use crate::journal::ConnectionJournal;

use super::JobFactsReadContext;
use super::values::positive_u64;

pub(super) struct LaunchFacts {
    pub(super) profile_generation: String,
    pub(super) profile_digest: String,
    pub(super) sample_generation: String,
    pub(super) observed_at: u64,
    pub(super) binding_digest: String,
    pub(super) scale_set_id: i64,
    pub(super) guest_boot_id: String,
}

pub(super) async fn read_launch_facts(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    context: &JobFactsReadContext<'_>,
) -> Result<LaunchFacts, HostError> {
    let mut rows = connection.query(
        "SELECT provisioning.profile_generation_id, provisioning.profile_digest, provisioning.sample_generation_id, provisioning.observed_at_unix_ms, reservation.binding_digest, intent.scale_set_id, state.guest_boot_id FROM capacity_reservation_provisioning AS provisioning JOIN capacity_reservations AS reservation ON reservation.id = provisioning.reservation_id JOIN connection_intents AS intent ON intent.id = provisioning.operation_id AND intent.connection_id = reservation.connection_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id AND jit.state = 'bound' AND jit.runner_id = ?4 AND jit.runner_name = ?5 AND jit.scale_set_id = ?6 JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id WHERE reservation.id = ?1 AND reservation.connection_id = ?2 AND provisioning.operation_id = ?3 AND reservation.state = 'reserved' AND intent.state = 'pending' AND intent.cleanup_proven = 0 AND intent.runner_completed = 0 AND intent.launch_phase = 'jit_bound' AND state.resources_bound = 1 AND state.bootstrap_start_state = 'started' AND state.jit_state = 'bound' AND state.publication_state = 'requested' AND state.delivery_state = 'not_requested' AND state.storage_cleanup_proven = 0 AND state.instance_binding_digest = ?7 AND state.runner_container_id = ?8 AND state.dind_container_id = ?9 AND state.guest_boot_id != ''",
        (context.reservation_id, context.connection_id, context.operation_id, context.runner_id, context.runner_name, context.scale_set_id, context.instance_binding_digest, context.runner_container_id, context.dind_container_id),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let facts = LaunchFacts {
        profile_generation: row.get(0).map_err(|_| HostError::Journal)?,
        profile_digest: row.get(1).map_err(|_| HostError::Journal)?,
        sample_generation: row.get(2).map_err(|_| HostError::Journal)?,
        observed_at: positive_u64(row.get(3).map_err(|_| HostError::Journal)?)?,
        binding_digest: row.get(4).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(5).map_err(|_| HostError::Journal)?,
        guest_boot_id: row.get(6).map_err(|_| HostError::Journal)?,
    };
    validate_launch_facts(&mut rows, journal, context, &facts).await?;
    ensure_no_execution_proof(connection, context).await?;
    Ok(facts)
}

async fn validate_launch_facts(
    rows: &mut turso::Rows,
    journal: &ConnectionJournal,
    context: &JobFactsReadContext<'_>,
    facts: &LaunchFacts,
) -> Result<(), HostError> {
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    if row.is_some()
        || facts.profile_generation != context.profile_generation_id
        || facts.profile_digest != context.profile_digest
        || facts.sample_generation != context.sample_generation_id
        || facts.binding_digest != journal.binding_digest()
        || facts.scale_set_id != context.scale_set_id
        || facts.guest_boot_id.is_empty()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn ensure_no_execution_proof(
    connection: &turso::Connection,
    context: &JobFactsReadContext<'_>,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM connection_execution_proofs WHERE intent_id = ?1 AND reservation_id = ?2 AND connection_id = ?3)",
            (context.operation_id, context.reservation_id, context.connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let exists = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() || exists != 0 {
        return Err(HostError::Identity);
    }
    Ok(())
}
