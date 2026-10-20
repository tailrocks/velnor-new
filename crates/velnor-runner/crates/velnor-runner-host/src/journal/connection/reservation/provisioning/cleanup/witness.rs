//! Record the worker data that the cleanup observer will check.

use crate::error::HostError;
use crate::launch::bootstrap::VerifiedStorageResourceBinding;
use sha2::{Digest, Sha256};
use turso::Connection;
use velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding;

#[path = "witness/read.rs"]
mod read;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageCleanupObserverWitness {
    reservation_id: i64,
    connection_id: String,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    binding: CleanupObserverBinding,
    binding_sha256: String,
}

impl StorageCleanupObserverWitness {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
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

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn binding(&self) -> &CleanupObserverBinding {
        &self.binding
    }

    pub(crate) fn binding_sha256(&self) -> &str {
        &self.binding_sha256
    }
}

pub(super) async fn insert(
    connection: &Connection,
    connection_id: &str,
    reservation_id: i64,
    verified: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if verified.reservation_id() != reservation_id {
        return Err(HostError::Identity);
    }
    let binding = verified.cleanup_observer_binding();
    velnor_resource_probe::cleanup_observer_wire::validate_binding(binding)
        .map_err(|_| HostError::Identity)?;
    let binding_json = canonical_json(binding)?;
    let binding_sha256 = binding_sha256(binding_json.as_bytes());
    let changed = connection
        .execute(
            "INSERT INTO capacity_reservation_cleanup_observer_witnesses (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, runner_container_id, dind_container_id, instance_binding_digest, guest_boot_id, binding_json, binding_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            (
                reservation_id,
                connection_id,
                verified.operation_id(),
                verified.engine_id(),
                verified.docker_root_digest(),
                verified.runner_container_id(),
                verified.dind_container_id(),
                verified.instance_binding_digest(),
                binding.expected_guest_boot_id.as_str(),
                binding_json.as_str(),
                binding_sha256.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn read(
    connection: &Connection,
    connection_id: &str,
    reservation_id: i64,
) -> Result<Option<StorageCleanupObserverWitness>, HostError> {
    read::read(connection, connection_id, reservation_id).await
}

pub(super) fn binding_sha256(canonical_binding: &[u8]) -> String {
    hex(&Sha256::digest(canonical_binding))
}

pub(super) fn canonical_json<T: serde::Serialize>(value: &T) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostError::Journal)?;
    String::from_utf8(bytes).map_err(|_| HostError::Journal)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}
