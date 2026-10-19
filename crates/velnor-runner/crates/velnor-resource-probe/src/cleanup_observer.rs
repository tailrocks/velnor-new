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

/// Maximum number of recorded volume mounts.
pub const MAX_CLEANUP_VOLUMES: usize = 5;
/// Maximum number of temporary filesystem devices used by these workers.
pub const OWNED_TMPFS_DEVICE_COUNT: usize = 8;
/// Maximum size of the request bytes.
pub const CLEANUP_REQUEST_LIMIT: usize = 32 * 1024;

/// Make sure that the request fields are correct.
/// Make sure that the digest is equal to the digest of the binding data.
///
/// # Errors
///
/// Returns an error if a request field or the binding digest is incorrect.
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

/// Find identities of Velnor resources in process and mount data at the specified Linux paths.
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
