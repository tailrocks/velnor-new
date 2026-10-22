//! Bootstrap receipts with private production constructors.
//!
//! Only the Docker adapter can make a production bootstrap receipt. Callers can
//! read its identity fields for journal comparison.

pub(crate) mod adapter;
mod cleanup;
mod containers;
mod delivery;
mod pre_jit_recovery;
mod publication;
mod storage;

pub(crate) use adapter::PreparedAcquiredBootstrap;
pub(crate) use cleanup::{
    NativeCleanupRuntime, PendingStorageCleanupWitness, VerifiedStorageCleanup,
};
pub(crate) use containers::{
    BootstrapContainerRole, BootstrapPairInput, BootstrapStartReceipt, PendingBootstrapPair,
    StartedBootstrapPair, VerifiedBootstrapContainerCreated, VerifiedBootstrapMountHandoff,
    VerifiedCreatedStorageObject, VerifiedStorageNetworkCreated, VerifiedStorageNetworkRemoved,
    create_owned_network, remove_owned_network, remove_owned_network_for_retirement,
    start_bootstrap_pair,
};
pub(crate) use delivery::ListenerDeliveryReceipt;
#[cfg(test)]
pub(crate) use pre_jit_recovery::remove_bootstrap_with_claim;
pub(crate) use pre_jit_recovery::{
    PreJitCleanupPhase, PreJitCleanupPhaseProof, PreJitCleanupSettlementProof,
};
pub(crate) use pre_jit_recovery::{
    recover_pre_jit_reservation, remove_bootstrap_after_terminal_failure,
};
pub(crate) use publication::JobFactsPublication;
#[cfg(test)]
pub(crate) use publication::JobFactsPublicationFixtureInput;
pub(crate) use publication::job_facts_publication;
pub(in crate::launch) use storage::StorageObservationContext;
pub(crate) use storage::VerifiedStorageResourceBinding;
pub(in crate::launch) use storage::observe_storage_resources_with_provider;
pub(crate) use storage::{ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole};

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct RootBackingMappingFixtureInput<'a> {
    pub(crate) reservation_id: i64,
    pub(crate) operation_id: i64,
    pub(crate) connection_id: &'a str,
    pub(crate) engine_id: &'a str,
    pub(crate) docker_root_digest: &'a str,
    pub(crate) sample_generation_id: &'a str,
    pub(crate) role: crate::journal::StorageRole,
    pub(crate) container_id: &'a str,
    pub(crate) image_runtime_id: &'a str,
    pub(crate) image_source_sha: &'a str,
    pub(crate) attachment: &'a ObservedStorageAttachment,
    pub(crate) backing_filesystem_instance_digest: &'a str,
}

#[cfg(test)]
pub(crate) struct VerifiedStorageResourceBindingFixture {
    pub(crate) reservation_id: i64,
    pub(crate) operation_id: i64,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) sample_generation_id: String,
    pub(crate) profile_generation_id: String,
    pub(crate) profile_digest: String,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) connection_id: String,
    pub(crate) instance_binding_digest: String,
    pub(crate) roles: Vec<ObservedStorageRole>,
    pub(crate) attachments: Vec<ObservedStorageAttachment>,
    pub(crate) cgroups: Vec<ObservedContainerCgroup>,
    pub(crate) guest_boot_id: String,
}

#[cfg(test)]
mod test_fixtures;

#[cfg(test)]
pub(crate) use test_fixtures::{
    test_bootstrap_start_receipt, test_created_bootstrap_container, test_created_storage_object,
    test_job_facts_publication, test_listener_delivery_receipt, test_observed_container_cgroup,
    test_observed_storage_attachment, test_observed_storage_role, test_verified_storage_cleanup,
    test_verified_storage_resource_binding,
};

#[cfg(test)]
pub(crate) use containers::test_bootstrap_mount_handoff;

#[cfg(test)]
pub(crate) use cleanup::runtime_tests::docker::OwnedContainerMismatch;
#[cfg(test)]
pub(crate) use cleanup::runtime_tests::{
    CleanupRuntimeFixture, cleanup_runtime_fixture, cleanup_runtime_fixture_for_base,
    cleanup_runtime_fixture_for_base_with_capacity, verify_storage_cleanup_for_scope,
};
