// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ProbeError;
#[cfg(target_os = "linux")]
mod scan;

pub use crate::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord,
    CleanupObserverRequest, CleanupVolume, NamespaceIdentity, ProcessIdentity,
};

pub const MAX_CLEANUP_VOLUMES: usize = 5;

pub const OWNED_TMPFS_DEVICE_COUNT: usize = 8;

pub const CLEANUP_REQUEST_LIMIT: usize = 32 * 1024;

pub fn validate_request(request: &CleanupObserverRequest) -> Result<(), ProbeError> {
    crate::cleanup_observer_wire::validate_request(request).map_err(ProbeError::Invalid)
}

#[cfg(target_os = "linux")]
pub fn observe_cleanup(
    request: CleanupObserverRequest,
) -> Result<CleanupObserverRecord, ProbeError> {
    validate_request(&request)?;
    scan::observe(request)
}
