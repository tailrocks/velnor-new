use crate::error::HostError;
use crate::journal::Journal;

use super::admission::JOB_LIMIT;
use super::helpers::{
    Scratch, binding, created_scope, prepare_physical_initializer, provision_physical_intent,
};

#[tokio::test]
async fn cleanup_identity_requires_a_bound_held_reservation_and_reads_exact_owned_rows()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(crate::journal::CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: 40 * 1024 * 1024 * 1024,
            ..JOB_LIMIT
        })
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

    match identity.topology_version() {
        1 => assert_eq!(identity.volumes().len(), 4),
        2 => assert_eq!(identity.volumes().len(), 5),
        _ => return Err(HostError::Journal),
    }
    assert_eq!(
        identity.mounts().len(),
        if identity.topology_version() == 1 {
            14
        } else {
            16
        }
    );
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
    Ok(())
}
