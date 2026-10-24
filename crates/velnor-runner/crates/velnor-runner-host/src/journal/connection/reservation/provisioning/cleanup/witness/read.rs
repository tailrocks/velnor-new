use super::{StorageCleanupObserverWitness, binding_sha256, canonical_json};
use crate::error::HostError;
use turso::Connection;
use velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding;

pub(super) async fn read(
    connection: &Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<Option<StorageCleanupObserverWitness>, HostError> {
    if reservation_id <= 0 {
        return Err(HostError::Identity);
    }
    let mut rows = connection
        .query(
            "SELECT witness.connection_id, witness.operation_id, witness.engine_id, witness.docker_root_digest, witness.runner_container_id, witness.dind_container_id, witness.instance_binding_digest, witness.guest_boot_id, witness.binding_json, witness.binding_sha256, reservation.state, intent.state, intent.cleanup_proven, state.bootstrap_start_state, state.resources_bound, state.jit_state, state.storage_cleanup_proven, state.instance_binding_digest, state.guest_boot_id, state.runner_container_id, state.dind_container_id, reservation.intent_id, reservation.engine_id, reservation.docker_root_digest FROM capacity_reservation_cleanup_observer_witnesses AS witness JOIN capacity_reservations AS reservation ON reservation.id = witness.reservation_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id JOIN connection_intents AS intent ON intent.id = provisioning.operation_id AND intent.connection_id = reservation.connection_id WHERE witness.reservation_id = ?1 AND reservation.connection_id = ?2",
            (reservation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    read_witness(&mut rows, connection_id, reservation_id).await
}

async fn read_witness(
    rows: &mut turso::Rows,
    connection_id: &str,
    reservation_id: i64,
) -> Result<Option<StorageCleanupObserverWitness>, HostError> {
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let witness_connection: String = row.get(0).map_err(|_| HostError::Journal)?;
    let operation_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(2).map_err(|_| HostError::Journal)?;
    let docker_root_digest: String = row.get(3).map_err(|_| HostError::Journal)?;
    let runner_container_id: String = row.get(4).map_err(|_| HostError::Journal)?;
    let dind_container_id: String = row.get(5).map_err(|_| HostError::Journal)?;
    let instance_binding_digest: String = row.get(6).map_err(|_| HostError::Journal)?;
    let guest_boot_id: String = row.get(7).map_err(|_| HostError::Journal)?;
    let binding_json: String = row.get(8).map_err(|_| HostError::Journal)?;
    let digest: String = row.get(9).map_err(|_| HostError::Journal)?;
    let reservation_state: String = row.get(10).map_err(|_| HostError::Journal)?;
    let intent_state: String = row.get(11).map_err(|_| HostError::Journal)?;
    let intent_cleanup: i64 = row.get(12).map_err(|_| HostError::Journal)?;
    let bootstrap_state: String = row.get(13).map_err(|_| HostError::Journal)?;
    let resources_bound: i64 = row.get(14).map_err(|_| HostError::Journal)?;
    let jit_state: String = row.get(15).map_err(|_| HostError::Journal)?;
    let cleanup_proven: i64 = row.get(16).map_err(|_| HostError::Journal)?;
    let current_digest: Option<String> = row.get(17).map_err(|_| HostError::Journal)?;
    let current_boot_id: Option<String> = row.get(18).map_err(|_| HostError::Journal)?;
    let current_runner: Option<String> = row.get(19).map_err(|_| HostError::Journal)?;
    let current_dind: Option<String> = row.get(20).map_err(|_| HostError::Journal)?;
    let intent_id: i64 = row.get(21).map_err(|_| HostError::Journal)?;
    let current_engine: String = row.get(22).map_err(|_| HostError::Journal)?;
    let current_root: String = row.get(23).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || witness_connection != connection_id
        || operation_id != intent_id
        || engine_id != current_engine
        || docker_root_digest != current_root
        || current_digest.as_deref() != Some(instance_binding_digest.as_str())
        || current_boot_id.as_deref() != Some(guest_boot_id.as_str())
        || current_runner.as_deref() != Some(runner_container_id.as_str())
        || current_dind.as_deref() != Some(dind_container_id.as_str())
        || !matches!(
            reservation_state.as_str(),
            "reserved" | "uncertain" | "recovering"
        )
        || !matches!(intent_state.as_str(), "pending" | "uncertain" | "done")
        || intent_cleanup != 0
        || bootstrap_state != "started"
        || resources_bound != 1
        || !matches!(
            jit_state.as_str(),
            "not_requested" | "requested" | "bound" | "uncertain" | "cleaned"
        )
        || cleanup_proven != 0
    {
        return Err(HostError::Identity);
    }
    if binding_json.len() > 32768 || digest != binding_sha256(binding_json.as_bytes()) {
        return Err(HostError::Journal);
    }
    let binding: CleanupObserverBinding =
        serde_json::from_str(&binding_json).map_err(|_| HostError::Journal)?;
    velnor_resource_probe::cleanup_observer_wire::validate_binding(&binding)
        .map_err(|_| HostError::Journal)?;
    if canonical_json(&binding)? != binding_json || binding.expected_guest_boot_id != guest_boot_id
    {
        return Err(HostError::Journal);
    }
    Ok(Some(StorageCleanupObserverWitness {
        reservation_id,
        connection_id: witness_connection,
        operation_id,
        engine_id,
        docker_root_digest,
        runner_container_id,
        dind_container_id,
        instance_binding_digest,
        binding,
        binding_sha256: digest,
    }))
}
