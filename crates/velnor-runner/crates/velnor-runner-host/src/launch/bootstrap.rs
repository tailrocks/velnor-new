//! Bootstrap receipts with private production constructors.
//!
//! Only the Docker adapter can make a production bootstrap receipt. Callers can
//! read its identity fields for journal comparison.

pub(crate) mod adapter;
mod cleanup;
mod containers;
mod delivery;
mod publication;
mod storage;

pub(crate) use adapter::PreparedAcquiredBootstrap;
pub(crate) use cleanup::{
    NativeCleanupRuntime, PendingStorageCleanupWitness, VerifiedStorageCleanup,
};
pub(crate) use containers::{
    BootstrapContainerRole, BootstrapStartReceipt, PendingBootstrapPair, StartedBootstrapPair,
    VerifiedBootstrapContainerCreated, VerifiedBootstrapMountHandoff, VerifiedCreatedStorageObject,
    VerifiedStorageNetworkCreated, VerifiedStorageNetworkRemoved, create_owned_network,
    remove_owned_network, start_bootstrap_pair,
};
pub(crate) use delivery::ListenerDeliveryReceipt;
pub(crate) use publication::JobFactsPublication;
pub(crate) use publication::job_facts_publication;
pub(crate) use storage::VerifiedStorageResourceBinding;
pub(in crate::launch) use storage::observe_storage_resources_with_provider;
pub(crate) use storage::{ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole};

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
pub(crate) use cleanup::runtime_tests::{
    CleanupRuntimeFixture, cleanup_runtime_fixture, cleanup_runtime_fixture_for_base,
};
