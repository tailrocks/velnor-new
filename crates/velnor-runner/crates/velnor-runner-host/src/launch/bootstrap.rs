//! Bootstrap receipts with private production constructors.
//!
//! Only the Docker adapter can make a production bootstrap receipt. Callers can
//! read its identity fields for journal comparison.

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
pub(crate) use storage::VerifiedStorageResourceBinding;
#[cfg(test)]
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
