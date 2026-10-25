const CLEANUP_GUARDS: [(&str, &str); 7] = [
    (
        "provisioning_storage_cleanup_identity_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_identity_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id WHERE topology.reservation_id = NEW.reservation_id AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
    (
        "provisioning_storage_cleanup_shared_run_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_shared_run_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id JOIN capacity_reservation_provisioning_objects AS object ON object.reservation_id = topology.reservation_id AND object.role = 'shared-run' AND object.create_state = 'created' AND object.docker_id = object.docker_name AND cleanup.shared_run_volume = object.docker_name WHERE topology.reservation_id = NEW.reservation_id AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
    (
        "provisioning_storage_cleanup_runner_home_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_runner_home_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id JOIN capacity_reservation_provisioning_objects AS object ON object.reservation_id = topology.reservation_id AND object.role = 'runner-home' AND object.create_state = 'created' AND object.docker_id = object.docker_name AND cleanup.runner_home_volume = object.docker_name WHERE topology.reservation_id = NEW.reservation_id AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
    (
        "provisioning_storage_cleanup_runner_work_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_runner_work_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id JOIN capacity_reservation_provisioning_objects AS object ON object.reservation_id = topology.reservation_id AND object.role = 'runner-work' AND object.create_state = 'created' AND object.docker_id = object.docker_name AND cleanup.runner_work_volume = object.docker_name WHERE topology.reservation_id = NEW.reservation_id AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
    (
        "provisioning_storage_cleanup_dind_data_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_dind_data_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id JOIN capacity_reservation_provisioning_objects AS object ON object.reservation_id = topology.reservation_id AND object.role = 'dind-data' AND object.create_state = 'created' AND object.docker_id = object.docker_name AND cleanup.dind_data_volume = object.docker_name WHERE topology.reservation_id = NEW.reservation_id AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
    (
        "provisioning_storage_cleanup_topology_v1_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_topology_v1_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology WHERE reservation_id = NEW.reservation_id AND topology_version = 1) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id WHERE topology.reservation_id = NEW.reservation_id AND topology.topology_version = 1 AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1 AND cleanup.metadata_volume IS NULL AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id) = 4 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id AND create_state = 'created' AND docker_id = docker_name) = 4) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
    (
        "provisioning_storage_cleanup_topology_v2_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_storage_cleanup_topology_v2_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state WHEN OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology WHERE reservation_id = NEW.reservation_id AND topology_version = 2) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_provisioning_topology AS topology JOIN capacity_reservation_provisioning_cleanups AS cleanup ON cleanup.reservation_id = topology.reservation_id WHERE topology.reservation_id = NEW.reservation_id AND topology.topology_version = 2 AND cleanup.runner_container_id = NEW.runner_container_id AND cleanup.dind_container_id = NEW.dind_container_id AND cleanup.instance_binding_digest = NEW.instance_binding_digest AND cleanup.role_mask = 2047 AND cleanup.containers_absent = 1 AND cleanup.volumes_absent = 1 AND cleanup.mounts_detached = 1 AND cleanup.cgroups_absent = 1 AND cleanup.metadata_volume = (SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id AND role = 'engine-metadata' AND create_state = 'created' AND docker_id = docker_name AND publication_policy_digest IS NOT NULL) AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id) = 5 AND (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = NEW.reservation_id AND create_state = 'created' AND docker_id = docker_name) = 5) BEGIN SELECT RAISE(ABORT, 'The cleanup proof does not match the owned objects.'); END",
    ),
];

pub(super) async fn replace_cleanup_guards(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute(
            "DROP TRIGGER IF EXISTS provisioning_storage_cleanup_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for (name, _) in CLEANUP_GUARDS {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (_, statement) in CLEANUP_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
