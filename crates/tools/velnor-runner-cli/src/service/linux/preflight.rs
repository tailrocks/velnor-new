//! Validate the loaded controller unit before systemd enters `ExecStart`.
//!
//! This command runs from the unit's `ExecStartPre` path on boot, direct
//! `systemctl start`, and restart. It deliberately checks the loaded unit
//! contract rather than its activation state: the unit is normally
//! `activating` while its own preflight runs.

#[cfg(test)]
mod tests;

use super::{
    Manager, ServiceFault, read_identity_snapshot, read_snapshot, verify_identity_contract,
    verify_load_credential, verify_package_contract, verify_service_environment,
};
use std::path::Path;
use velnor_runner_host::{HostError, HostPlatform, MAX_LINUX_DRAIN_TIMEOUT_SECS};

use super::contract::configured_drain_timeout_with;

/// Verify effective systemd properties before the daemon can admit work.
///
/// The caller validates the fixed package paths and reads the bounded Linux
/// drain deadline before invoking this helper. `verify_package_contract` is the
/// single package contract for the pinned 1860-second systemd stop timeout; the
/// 60-second allowance above the 1800-second config cap is explicit but not
/// measured and does not qualify package activation.
pub(super) fn verify_loaded_unit(
    manager: &mut impl Manager,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    if drain_timeout_secs == 0 || drain_timeout_secs > MAX_LINUX_DRAIN_TIMEOUT_SECS {
        return Err(ServiceFault::InvalidConfig);
    }
    let service = read_snapshot(manager)?;
    verify_package_contract(&service, drain_timeout_secs)?;
    verify_load_credential(manager)?;
    verify_service_environment(manager)?;
    let identity = read_identity_snapshot(manager)?;
    verify_identity_contract(&identity)
}

/// Read the protected Linux config before contacting systemd, then verify the
/// package-owned unit against the deadline from that exact accepted file.
pub(super) fn verify_loaded_unit_for_config(
    manager: &mut impl Manager,
    config_path: &Path,
    read_config: impl FnOnce(&Path, HostPlatform) -> Result<Option<String>, HostError>,
) -> Result<(), ServiceFault> {
    let drain_timeout_secs = configured_drain_timeout_with(config_path, read_config)?;
    verify_loaded_unit(manager, drain_timeout_secs)
}
