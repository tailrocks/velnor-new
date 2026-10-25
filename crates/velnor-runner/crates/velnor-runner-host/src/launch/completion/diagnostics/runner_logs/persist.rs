#[path = "persist/atomic.rs"]
mod atomic;

#[cfg(test)]
#[path = "persist/atomic_tests.rs"]
mod atomic_tests;

use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::error::HostError;

pub(super) fn write_projection(
    scope: &OwnedFd,
    name: &str,
    bytes: &[u8],
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    atomic::write_projection(scope, name, bytes, started, deadline, cancelled)
}

pub(super) fn validate_private_directory(directory: &OwnedFd) -> Result<(), HostError> {
    atomic::validate_private_directory(directory)
}
