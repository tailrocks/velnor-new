use crate::error::HostError;
use crate::launch::bootstrap::BootstrapContainerRole;

use super::StorageProvisioningPlan;

mod create;
mod start;

/// One durable container create attempt.
pub(crate) struct BootstrapContainerCreateClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: BootstrapContainerRole,
    container_name: String,
    storage_plan: StorageProvisioningPlan,
}

impl BootstrapContainerCreateClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) const fn role(&self) -> BootstrapContainerRole {
        self.role
    }

    pub(crate) fn container_name(&self) -> &str {
        &self.container_name
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn storage_plan(&self) -> &StorageProvisioningPlan {
        &self.storage_plan
    }
}

/// Operation claim for starting the JIT-withheld bootstrap container pair.
pub(crate) struct BootstrapStartClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    storage_plan: StorageProvisioningPlan,
}

impl BootstrapStartClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn storage_plan(&self) -> &StorageProvisioningPlan {
        &self.storage_plan
    }
}

async fn objects_are_created(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects AS object JOIN capacity_reservation_provisioning_topology AS topology ON topology.reservation_id = object.reservation_id WHERE object.reservation_id = ?1 AND topology.topology_version = 2 AND object.create_state = 'created' AND object.docker_id = object.docker_name AND (object.role != 'engine-metadata' OR object.publication_policy_digest IS NOT NULL)",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(count == 5)
}

fn invalid_resource_id(value: &str) -> bool {
    value.is_empty() || value.len() > 256 || value.bytes().any(|byte| byte.is_ascii_control())
}
