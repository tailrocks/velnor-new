//! Opaque receipts for the JIT-withheld worker bootstrap sequence.
//!
//! Production construction belongs to the Docker adapter. Callers can read
//! bounded identity fields for journal comparison, but cannot manufacture a
//! production receipt.

mod cleanup;
mod containers;
mod delivery;
mod publication;
mod storage;

pub(crate) use cleanup::VerifiedStorageCleanup;
pub(crate) use containers::{
    BootstrapContainerRole, BootstrapStartReceipt, VerifiedBootstrapContainerCreated,
    VerifiedCreatedStorageObject,
};
pub(crate) use delivery::ListenerDeliveryReceipt;
pub(crate) use publication::JobFactsPublication;
pub(crate) use storage::{
    ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole,
    VerifiedStorageResourceBinding,
};

#[cfg(test)]
mod test_fixtures;

#[cfg(test)]
pub(crate) use test_fixtures::{
    test_bootstrap_start_receipt, test_created_bootstrap_container, test_created_storage_object,
    test_job_facts_publication, test_listener_delivery_receipt, test_observed_container_cgroup,
    test_observed_storage_attachment, test_observed_storage_role, test_verified_storage_cleanup,
    test_verified_storage_resource_binding,
};
