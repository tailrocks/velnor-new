mod network;
mod pair;
mod storage_object;
#[cfg(test)]
#[path = "containers/test_support.rs"]
mod test_support;
pub(crate) use network::{
    remove_owned_network, remove_owned_network_for_jit_conflict,
    remove_owned_network_for_retirement,
};
#[cfg(test)]
pub(crate) use test_support::{
    test_bootstrap_mount_handoff, test_bootstrap_start_receipt, test_created_bootstrap_container,
    test_created_storage_object,
};

pub(in crate::launch::bootstrap) use network::validate_pre_jit_network;
pub(crate) use network::{
    VerifiedStorageNetworkCreated, VerifiedStorageNetworkRemoved, create_owned_network,
};
pub(crate) use pair::{
    BootstrapPairInput, PendingBootstrapPair, StartedBootstrapPair, start_bootstrap_pair,
};
pub(in crate::launch::bootstrap) use pair::{
    PairCreate, PairSpecContext, create_spec, create_spec_sha256, verify_pre_jit_created,
};
pub(in crate::launch::bootstrap) use storage_object::verify_pre_jit_volume;
pub(crate) use storage_object::{create_metadata_storage_object, create_storage_object};

/// Proof that both workers mounted the exact initialized storage objects.
pub(crate) struct VerifiedBootstrapMountHandoff {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    layout_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    mount_evidence_digest: String,
}

impl VerifiedBootstrapMountHandoff {
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }
    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }
    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }
    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }
    pub(crate) fn mount_evidence_digest(&self) -> &str {
        &self.mount_evidence_digest
    }
}

/// One of the two fixed bootstrap containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootstrapContainerRole {
    /// Unprivileged Actions runner, started with stdin open and no JIT bytes.
    Runner,
    /// Private privileged `DinD` service for this runner only.
    Dind,
}

/// Exact Docker create response and inspect evidence for one bootstrap container.
pub(crate) struct VerifiedBootstrapContainerCreated {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: BootstrapContainerRole,
    container_name: String,
    container_id: String,
}

impl VerifiedBootstrapContainerCreated {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn role(&self) -> BootstrapContainerRole {
        self.role
    }

    pub(crate) fn container_name(&self) -> &str {
        &self.container_name
    }

    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }
}

/// Exact returned and inspected identity of one named tmpfs volume object.
pub(crate) struct VerifiedCreatedStorageObject {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: crate::journal::StorageRole,
    docker_name: String,
    docker_id: String,
}

impl VerifiedCreatedStorageObject {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) const fn role(&self) -> crate::journal::StorageRole {
        self.role
    }

    pub(crate) fn docker_name(&self) -> &str {
        &self.docker_name
    }

    /// Docker volumes have no separate permanent ID. This value is the inspected volume name.
    pub(crate) fn docker_id(&self) -> &str {
        &self.docker_id
    }
}

/// The pair is running while the official Runner listener remains blocked on
/// its open stdin. This receipt never includes JIT bytes.
pub(crate) struct BootstrapStartReceipt {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    runner_stdin_open: bool,
    jit_withheld: bool,
}

impl BootstrapStartReceipt {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) const fn runner_stdin_open(&self) -> bool {
        self.runner_stdin_open
    }

    pub(crate) const fn jit_withheld(&self) -> bool {
        self.jit_withheld
    }
}
