use sha2::{Digest, Sha256};
use velnor_resource_probe::cleanup_observer_wire::{
    CleanupObserverRecord, CleanupObserverRequest, NamespaceIdentity, docker_root_matches,
};

use crate::error::HostError;

const MAX_RECORD_BYTES: usize = 32 * 1024;

pub(super) fn parse_record(
    bytes: &[u8],
    request: &CleanupObserverRequest,
) -> Result<CleanupObserverRecord, HostError> {
    if !valid_framing(bytes) {
        return Err(HostError::Frame);
    }
    let body = &bytes[..bytes.len() - 1];
    let record: CleanupObserverRecord =
        serde_json::from_slice(body).map_err(|_| HostError::Frame)?;
    if serde_json::to_vec(&record).map_err(|_| HostError::Frame)? != body {
        return Err(HostError::Frame);
    }
    validate_record(&record, request)?;
    Ok(record)
}

fn valid_framing(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && bytes.len() <= MAX_RECORD_BYTES
        && bytes.last() == Some(&b'\n')
        && !bytes[..bytes.len() - 1].contains(&b'\n')
        && !bytes
            .iter()
            .any(|byte| byte.is_ascii_control() && *byte != b'\n')
}

fn validate_record(
    record: &CleanupObserverRecord,
    request: &CleanupObserverRequest,
) -> Result<(), HostError> {
    let expected_namespaces = [
        NamespaceIdentity {
            device: request.binding.runner.namespace_device,
            inode: request.binding.runner.namespace_inode,
        },
        NamespaceIdentity {
            device: request.binding.dind.namespace_device,
            inode: request.binding.dind.namespace_inode,
        },
    ];
    let expected_volumes = request
        .binding
        .volume_mountpoints
        .iter()
        .map(|volume| volume.mountpoint.clone())
        .collect::<Vec<_>>();
    if record.schema_version != 1
        || record.operation_id != request.operation_id
        || record.binding_sha256 != request.binding_sha256
        || record.guest_boot_id.as_deref() != Some(request.binding.expected_guest_boot_id.as_str())
        || !record
            .docker_root
            .as_ref()
            .is_some_and(|guest| docker_root_matches(guest, &request.binding.docker_root))
        || !record.complete
        || !record.owned_references_absent
        || record.owned_reference_count != 0
        || !record.issues.is_empty()
        || record.absent_volume_mountpoints != expected_volumes
        || record.absent_tmpfs_devices.as_slice() != request.binding.owned_tmpfs_devices
        || record.absent_mount_namespaces.as_slice() != expected_namespaces
        || record.absent_cgroup_paths.as_ref() != Some(&request.binding.cgroup_paths)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn binding_digest(
    binding: &velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding,
) -> Result<String, HostError> {
    let encoded = serde_json::to_vec(binding).map_err(|_| HostError::Frame)?;
    Ok(hex(&Sha256::digest(encoded)))
}

pub(super) fn request_digest(request_json: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-cleanup-observer-request-v1\0");
    digest.update(request_json);
    digest.update(b"\n");
    hex(&digest.finalize())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
#[path = "record/tests.rs"]
mod tests;
