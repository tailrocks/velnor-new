use crate::error::HostError;
use crate::journal::Journal;

use super::admission::JOB_LIMIT;
use super::helpers::{
    Scratch, binding, created_scope, prepare_physical_initializer, provision_physical_intent,
};

async fn add_excess_attachment(journal: &Journal, reservation_id: i64) -> Result<usize, HostError> {
    let connection = journal.connection().await?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_attachments (reservation_id, role, observer, container_id, mount_target, source_target, read_only, mount_id, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest) SELECT reservation_id, role, observer, container_id, '/unexpected-extra', source_target, read_only, mount_id + 1000, device_major, device_minor, statx_device_major, statx_device_minor, fs_type_magic, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = ?1 AND role = 'runner-home' AND observer = 'runner' LIMIT 1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    usize::try_from(count).map_err(|_| HostError::Journal)
}

#[tokio::test]
async fn cleanup_identity_requires_a_bound_held_reservation_and_reads_exact_owned_rows()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            crate::journal::CapacityLimits {
                jobs: 2,
                cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
                memory_bytes: 40 * 1024 * 1024 * 1024,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 100,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let scope = created_scope(&journal, &binding("cleanup-identity", 1_401)?, 2_401).await?;

    let unbound = prepare_physical_initializer(&scope, 'c').await?;
    assert!(scope.storage_cleanup_identity(unbound).await?.is_none());

    let provisioned = provision_physical_intent(&scope, 'd').await?;
    let identity = scope
        .storage_cleanup_identity(provisioned.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(identity.reservation_id(), provisioned.reservation_id);
    assert_eq!(identity.operation_id(), provisioned.intent_id);
    assert_eq!(identity.engine_id(), super::helpers::TEST_ENGINE_ID);
    assert_eq!(
        identity.docker_root_digest(),
        provisioned.docker_root_digest
    );
    assert_eq!(
        identity.runner_container_id(),
        provisioned.runner_container_id
    );
    assert_eq!(identity.dind_container_id(), provisioned.dind_container_id);
    assert_eq!(
        identity.instance_binding_digest(),
        provisioned.instance_binding_digest
    );
    assert_eq!(identity.private_volume(), format!("w{}", provisioned.nonce));
    assert!(!identity.guest_boot_id().is_empty());
    let connection = journal.connection().await?;
    connection
        .execute("DROP TRIGGER bound_storage_path_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let changed = connection
        .execute(
            "UPDATE capacity_reservation_bound_storage_paths SET statx_inode = 999 WHERE reservation_id = ?1 AND role = 'engine-metadata' AND mount_target = '/opt/velnor/job-facts'",
            [provisioned.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(changed, 2);
    connection
        .execute(
            "CREATE TRIGGER bound_storage_path_no_update BEFORE UPDATE ON capacity_reservation_bound_storage_paths BEGIN SELECT RAISE(ABORT, 'The observed storage path cannot change.'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    assert!(
        !crate::journal::connection::validate_resources_bound(
            &journal.connection().await?,
            scope.connection_id().as_str(),
            provisioned.intent_id,
        )
        .await?
    );

    match identity.topology_version() {
        1 => assert_eq!(identity.volumes().len(), 4),
        2 => assert_eq!(identity.volumes().len(), 5),
        _ => return Err(HostError::Journal),
    }
    let expected_mounts = if identity.topology_version() == 1 {
        14
    } else {
        crate::journal::connection::StorageRole::PHYSICAL
            .iter()
            .map(|role| role.attachments().len())
            .sum()
    };
    assert_eq!(identity.mounts().len(), expected_mounts);
    assert_eq!(identity.cgroups().len(), 2);
    assert!(identity.mounts().iter().any(|mount| {
        mount.role() == "engine-metadata"
            && mount.observer() == "probe"
            && mount.target() == "/velnor/docker-root"
            && mount.source() == Some("/var/lib/docker")
            && mount.read_only()
    }));
    assert!(
        identity
            .cgroups()
            .iter()
            .all(|cgroup| cgroup.memory_max_bytes() > 0 && !cgroup.path().is_empty())
    );
    assert_eq!(
        add_excess_attachment(&journal, provisioned.reservation_id).await?,
        expected_mounts + 1
    );
    assert!(matches!(
        scope
            .storage_cleanup_identity(provisioned.reservation_id)
            .await,
        Err(HostError::Journal)
    ));
    Ok(())
}

#[cfg(test)]
#[path = "cleanup_identity/legacy.rs"]
mod legacy;
