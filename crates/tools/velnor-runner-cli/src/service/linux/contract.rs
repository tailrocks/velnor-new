//! The package-owned systemd command and account contract.

use std::path::Path;

use velnor_runner_host::{
    HostConfig, HostError, HostPlatform, MAX_LINUX_DRAIN_TIMEOUT_SECS, read_host_config_file,
};

use super::systemd::{IdentityUnitSnapshot, StopTimeout, UnitSnapshot};
use super::{BINARY, CONFIG, IDENTITY_BINARY, IDENTITY_UNIT, STATE, ServiceFault, UNIT};

const SYSTEMD_STOP_ALLOWANCE_SECS: u64 = 60;
pub(super) const SYSTEMD_STOP_TIMEOUT_SECS: u64 =
    MAX_LINUX_DRAIN_TIMEOUT_SECS + SYSTEMD_STOP_ALLOWANCE_SECS;
pub(super) const SYSTEMD_STOP_TIMEOUT_USEC: u128 = SYSTEMD_STOP_TIMEOUT_SECS as u128 * 1_000_000;

pub(super) fn verify_package_contract(
    snapshot: &UnitSnapshot,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    if drain_timeout_secs == 0 || drain_timeout_secs > MAX_LINUX_DRAIN_TIMEOUT_SECS {
        return Err(ServiceFault::InvalidConfig);
    }
    if snapshot.load_state != "loaded"
        || snapshot.exec_start_pre.path != BINARY
        || snapshot.exec_start_pre.argv != preflight_argv()
        || snapshot.exec_start_pre.ignore_errors != "no"
        || snapshot.exec_start.path != BINARY
        || snapshot.exec_start.argv != start_argv()
        || snapshot.exec_start.ignore_errors != "no"
        || snapshot.exec_stop.path != BINARY
        || snapshot.exec_stop.argv != stop_argv()
        || snapshot.exec_stop.ignore_errors != "no"
        || snapshot.timeout_stop != StopTimeout::Finite(SYSTEMD_STOP_TIMEOUT_USEC)
        || snapshot.timeout_stop_failure_mode != "terminate"
        || snapshot.kill_signal != "15"
        || snapshot.kill_mode != "mixed"
        || snapshot.user != "velnor"
        || snapshot.group != "velnor"
        || snapshot.supplementary_groups != "docker"
        || snapshot.working_directory != STATE
        || snapshot.umask != "0077"
        || snapshot.no_new_privileges != "yes"
        || snapshot.protect_system != "strict"
        || snapshot.read_write_paths != [STATE]
        || !snapshot
            .requires
            .iter()
            .any(|unit| unit == "docker.service")
        || !snapshot.requires.iter().any(|unit| unit == IDENTITY_UNIT)
        || !snapshot
            .after
            .iter()
            .any(|unit| unit == "network-online.target")
        || !snapshot.after.iter().any(|unit| unit == "docker.service")
        || !snapshot.after.iter().any(|unit| unit == IDENTITY_UNIT)
        || snapshot.unit_type != "simple"
        || !snapshot.identity_marker_condition_matches
    {
        return Err(ServiceFault::ServiceContract);
    }
    Ok(())
}

pub(super) fn verify_identity_contract(
    snapshot: &IdentityUnitSnapshot,
) -> Result<(), ServiceFault> {
    if snapshot.load_state != "loaded"
        || snapshot.exec_start.path != IDENTITY_BINARY
        || snapshot.exec_start.argv != [IDENTITY_BINARY, "--verify"]
        || snapshot.exec_start.ignore_errors != "no"
        || snapshot.user != "root"
        || snapshot.group != "root"
        || snapshot.umask != "0077"
        || snapshot.no_new_privileges != "yes"
        || snapshot.protect_system != "strict"
        || snapshot.unit_type != "oneshot"
        || snapshot.remain_after_exit != "no"
        || !snapshot.before.iter().any(|unit| unit == UNIT)
        || !snapshot.identity_marker_condition_matches
    {
        return Err(ServiceFault::ServiceContract);
    }
    Ok(())
}

pub(super) fn configured_drain_timeout(config_path: &Path) -> Result<u64, ServiceFault> {
    configured_drain_timeout_with(config_path, read_host_config_file)
}

pub(super) fn configured_drain_timeout_with(
    config_path: &Path,
    read_config: impl FnOnce(&Path, HostPlatform) -> Result<Option<String>, HostError>,
) -> Result<u64, ServiceFault> {
    let text = read_config(config_path, HostPlatform::Linux)
        .map_err(|_| ServiceFault::InvalidConfig)?
        .ok_or(ServiceFault::InvalidConfig)?;
    let config = HostConfig::parse(&text).map_err(|_| ServiceFault::InvalidConfig)?;
    config
        .validate_for_host(HostPlatform::Linux)
        .map_err(|_| ServiceFault::InvalidConfig)?;
    config
        .drain_timeout_secs()
        .map_err(|_| ServiceFault::InvalidConfig)
}

pub(super) fn package_paths_supported(config_path: &Path, state_path: &Path) -> bool {
    config_path == Path::new(CONFIG) && state_path == Path::new(STATE)
}

pub(super) fn start_argv() -> Vec<String> {
    [
        BINARY, "--config", CONFIG, "--state", STATE, "daemon", "run",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn preflight_argv() -> Vec<String> {
    [
        BINARY,
        "--config",
        CONFIG,
        "--state",
        STATE,
        "service",
        "preflight",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn stop_argv() -> Vec<String> {
    [BINARY, "--config", CONFIG, "--state", STATE, "drain"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}
