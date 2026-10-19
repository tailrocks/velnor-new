//! Measure owned process, mount, and descriptor references before cleanup.

use sha2::{Digest, Sha256};

use crate::ProbeError;
mod request;
#[cfg(target_os = "linux")]
mod scan;
#[cfg(test)]
mod tests;

pub use crate::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord,
    CleanupObserverRequest, CleanupVolume, NamespaceIdentity, ProcessIdentity,
};

/// Bound the number of recorded volume mounts.
pub const MAX_CLEANUP_VOLUMES: usize = 5;
/// Bound the number of owned temporary filesystem devices.
pub const OWNED_TMPFS_DEVICE_COUNT: usize = 8;
/// Bound the canonical request size.
pub const CLEANUP_REQUEST_LIMIT: usize = 32 * 1024;

/// Validate the closed request and its canonical binding digest.
///
/// # Errors
///
/// Returns an error if the request fields or binding digest do not pass validation.
pub fn validate_request(request: &CleanupObserverRequest) -> Result<(), ProbeError> {
    request::validate(request)?;
    let bytes = serde_json::to_vec(&request.binding)
        .map_err(|_| ProbeError::Invalid("cleanup_binding_encode"))?;
    let digest = lower_hex(&Sha256::digest(bytes));
    if digest != request.binding_sha256 {
        return Err(ProbeError::Invalid("cleanup_binding_digest"));
    }
    Ok(())
}

/// Inspect fixed Linux process and mount data for references to owned resources.
///
/// # Errors
///
/// Returns an error if the request is invalid or the guest boot identity changes.
#[cfg(target_os = "linux")]
pub fn observe_cleanup(
    request: CleanupObserverRequest,
) -> Result<CleanupObserverRecord, ProbeError> {
    validate_request(&request)?;
    scan::observe(request)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
