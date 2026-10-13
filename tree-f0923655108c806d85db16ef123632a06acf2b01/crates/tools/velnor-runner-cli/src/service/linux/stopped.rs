//! Read-only postcondition used after the package manager requests a stop.

use std::path::Path;
use std::process::ExitCode;

use velnor_runner_host::{HostPlatform, read_validated_host_config_snapshot};

use super::{
    Manager, ServiceFault, Systemctl, ensure_no_pending_unit_job, is_stopped,
    read_identity_marker_condition, read_identity_snapshot, read_snapshot, read_unit_snapshot,
    verify_identity_contract, verify_load_credential, verify_package_contract,
    verify_service_environment,
};

pub(super) fn verify_stopped_for_config(config_path: &Path) -> ExitCode {
    let timeout = match validated_drain_timeout(config_path) {
        Ok(timeout) => timeout,
        Err(fault) => {
            eprintln!("{}", fault.message());
            return ExitCode::from(1);
        }
    };
    let mut manager = Systemctl;
    match verify_stopped(&mut manager, timeout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("{}", fault.message());
            ExitCode::from(1)
        }
    }
}

fn validated_drain_timeout(config_path: &Path) -> Result<u64, ServiceFault> {
    let snapshot = read_validated_host_config_snapshot(config_path, HostPlatform::Linux)
        .map_err(|_| ServiceFault::InvalidConfig)?
        .ok_or(ServiceFault::InvalidConfig)?;
    snapshot
        .config()
        .drain_timeout_secs()
        .map_err(|_| ServiceFault::InvalidConfig)
}

pub(super) fn verify_stopped(
    manager: &mut impl Manager,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    let snapshot = read_snapshot(manager)?;
    verify_stopped_snapshot(&snapshot, drain_timeout_secs)?;
    verify_load_credential(manager)?;
    verify_service_environment(manager)?;
    verify_identity_contract(&read_identity_snapshot(manager)?)?;
    ensure_no_pending_unit_job(manager)?;

    let final_snapshot = read_final_snapshot(manager)?;
    verify_stopped_snapshot(&final_snapshot, drain_timeout_secs)
}

fn read_final_snapshot(manager: &mut impl Manager) -> Result<super::UnitSnapshot, ServiceFault> {
    let identity_condition = read_identity_marker_condition(manager, super::UNIT_OBJECT_PATH)?;
    let mut snapshot = read_unit_snapshot(manager)?;
    snapshot.identity_marker_condition_matches = identity_condition;
    Ok(snapshot)
}

fn verify_stopped_snapshot(
    snapshot: &super::UnitSnapshot,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    verify_package_contract(snapshot, drain_timeout_secs)?;
    if snapshot.id.as_deref() != Some(super::UNIT) {
        return Err(ServiceFault::ServiceContract);
    }
    if !is_stopped(snapshot) || snapshot.result != "success" {
        return Err(ServiceFault::StopNotVerified);
    }
    match snapshot.job.as_deref() {
        Some("") => Ok(()),
        Some(_) => Err(ServiceFault::PendingJob),
        None => Err(ServiceFault::UnknownState),
    }
}
