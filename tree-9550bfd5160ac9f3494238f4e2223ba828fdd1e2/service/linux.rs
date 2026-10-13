//! systemd lifecycle commands for the package-owned controller unit.

use std::io;
use std::path::Path;
use std::process::ExitCode;

use crate::args::ServiceAction;
use conditions::read_identity_marker_condition;
use systemd::{IdentityUnitSnapshot, UnitSnapshot, parse_identity_snapshot, parse_snapshot};
#[cfg(test)]
use systemd::{StopTimeout, parse_stop_timeout, parse_timespan_usec};
use velnor_runner_host::read_host_config_file;

mod conditions;
mod contract;
mod preflight;
mod process;
mod stop;
mod stopped;
mod systemd;

use contract::{
    configured_drain_timeout, package_paths_supported, verify_identity_contract,
    verify_package_contract,
};
#[cfg(test)]
use contract::{preflight_argv, start_argv, stop_argv};
use process::Systemctl;

const UNIT: &str = "velnor-host.service";
const IDENTITY_UNIT: &str = "velnor-host-identity-check.service";
const BINARY: &str = "/usr/bin/velnor-host";
const IDENTITY_BINARY: &str = "/usr/lib/velnor-host/account-check";
const IDENTITY_MARKER: &str = "/var/lib/velnor-host-package/identity";
const CONFIG: &str = "/etc/velnor-host/host.toml";
const STATE: &str = "/var/lib/velnor-host";
const SYSTEMD_BUS_NAME: &str = "org.freedesktop.systemd1";
const UNIT_OBJECT_PATH: &str = "/org/freedesktop/systemd1/unit/velnor_2dhost_2eservice";
const IDENTITY_OBJECT_PATH: &str =
    "/org/freedesktop/systemd1/unit/velnor_2dhost_2didentity_2dcheck_2eservice";
const SERVICE_INTERFACE: &str = "org.freedesktop.systemd1.Service";
const CREDENTIAL_PROPERTY: &str = "LoadCredential";
const EXPECTED_CREDENTIAL_PROPERTY: &str =
    "a(ss) 1 \"github-token\" \"/etc/velnor-host/github-token\"";
const ENVIRONMENT_PROPERTY: &str = "Environment";
const EXPECTED_ENVIRONMENT_PROPERTY: &str = "as 1 \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\"";
const SHOW_PROPERTIES: &str = concat!(
    "Id,LoadState,ActiveState,SubState,MainPID,ControlPID,Result,Job,ExecStartPre,ExecStart,ExecStop,",
    "TimeoutStopUSec,TimeoutStopFailureMode,KillSignal,KillMode,User,Group,SupplementaryGroups,WorkingDirectory,UMask,",
    "NoNewPrivileges,ProtectSystem,ReadWritePaths,Requires,After,Type"
);
const IDENTITY_SHOW_PROPERTIES: &str = concat!(
    "LoadState,ExecStart,User,Group,UMask,NoNewPrivileges,ProtectSystem,Type,",
    "RemainAfterExit,Before"
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagerOutput {
    success: bool,
    stdout: Vec<u8>,
}

trait Manager {
    fn systemctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput>;
    fn busctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceFault {
    Manager,
    UnitUnavailable,
    UnknownState,
    ServiceContract,
    IdentityUnitUnavailable,
    CredentialUnavailable,
    StartFailed,
    ServiceEnvironmentUnavailable,
    DrainUnavailable,
    DrainDeadline,
    StopCoordinatorRequired,
    StopNotVerified,
    PendingJob,
    PackageOwned,
    InvalidConfig,
}

impl ServiceFault {
    const fn message(self) -> &'static str {
        match self {
            Self::Manager => "systemd command failed",
            Self::UnitUnavailable => "velnor-host.service is not loaded",
            Self::UnknownState => "systemd did not provide a complete unit state",
            Self::ServiceContract => "velnor-host.service does not match the packaged contract",
            Self::IdentityUnitUnavailable => {
                "the required root identity preflight unit is unavailable"
            }
            Self::CredentialUnavailable => {
                "systemd did not prove the exact controller credential mapping"
            }
            Self::StartFailed => "velnor-host.service did not reach active/running",
            Self::ServiceEnvironmentUnavailable => {
                "systemd did not prove the exact packaged service environment"
            }
            Self::DrainUnavailable => {
                "service-user drain proof failed; systemd stop was not requested"
            }
            Self::DrainDeadline => "service-user drain or stop deadline elapsed; stop is unproven",
            Self::StopCoordinatorRequired => {
                "service stop requires the proof-backed Linux stop coordinator"
            }
            Self::StopNotVerified => "systemd did not prove a successful completed service stop",
            Self::PendingJob => "velnor-host.service still has a pending systemd job",
            Self::PackageOwned => "install and uninstall are managed by the system package",
            Self::InvalidConfig => "Linux host configuration is missing or invalid",
        }
    }
}

pub(crate) fn stop_for_disconnect(
    config_path: &Path,
    state_path: &Path,
    timeout_override: Option<u64>,
) -> ExitCode {
    if !package_paths_supported(config_path, state_path) {
        eprintln!("{}", ServiceFault::InvalidConfig.message());
        return ExitCode::from(1);
    }
    stop::stop_for_disconnect(config_path, state_path, timeout_override)
}

pub(super) fn service(action: ServiceAction, config_path: &Path, state_path: &Path) -> ExitCode {
    if matches!(action, ServiceAction::Status) {
        let mut manager = Systemctl::bounded_until(super::status_query_deadline());
        return match perform(action, &mut manager, 0) {
            Ok(()) => ExitCode::SUCCESS,
            Err(fault) => {
                eprintln!("{}", fault.message());
                ExitCode::from(1)
            }
        };
    }
    if matches!(action, ServiceAction::Install | ServiceAction::Uninstall) {
        eprintln!("{}", ServiceFault::PackageOwned.message());
        return ExitCode::from(1);
    }
    if !package_paths_supported(config_path, state_path) {
        eprintln!("{}", ServiceFault::InvalidConfig.message());
        return ExitCode::from(1);
    }
    if matches!(action, ServiceAction::Stop) {
        return stop::stop_for_config(config_path, state_path);
    }
    let mut manager = Systemctl::default();
    if matches!(action, ServiceAction::Preflight) {
        return match preflight::verify_loaded_unit_for_config(
            &mut manager,
            config_path,
            read_host_config_file,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(fault) => {
                eprintln!("{}", fault.message());
                ExitCode::from(1)
            }
        };
    }
    if matches!(action, ServiceAction::VerifyStopped) {
        return stopped::verify_stopped_for_config(config_path);
    }
    let drain_timeout_secs = match configured_drain_timeout(config_path) {
        Ok(timeout) => timeout,
        Err(fault) => {
            eprintln!("{}", fault.message());
            return ExitCode::from(1);
        }
    };
    match perform(action, &mut manager, drain_timeout_secs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("{}", fault.message());
            ExitCode::from(1)
        }
    }
}

pub(super) fn controller_service_state() -> super::ControllerServiceState {
    service_state_output(&mut Systemctl::bounded_until(super::status_query_deadline()))
        .map_or(super::ControllerServiceState::Unknown, |output| {
            super::systemd_service_state(output.success, &output.stdout)
        })
}

fn perform(
    action: ServiceAction,
    manager: &mut impl Manager,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    match action {
        ServiceAction::Status => service_status(manager),
        ServiceAction::Preflight => preflight::verify_loaded_unit(manager, drain_timeout_secs),
        ServiceAction::Install | ServiceAction::Uninstall => Err(ServiceFault::PackageOwned),
        ServiceAction::Start => start(manager, drain_timeout_secs),
        ServiceAction::Stop => Err(ServiceFault::StopCoordinatorRequired),
        ServiceAction::VerifyStopped => stopped::verify_stopped(manager, drain_timeout_secs),
    }
}

fn service_status(manager: &mut impl Manager) -> Result<(), ServiceFault> {
    let output = service_state_output(manager)?;
    println!("{}", service_status_line(output.success, &output.stdout)?);
    Ok(())
}

fn service_state_output(manager: &mut impl Manager) -> Result<ManagerOutput, ServiceFault> {
    let property = format!("--property={}", super::SERVICE_STATE_PROPERTIES);
    manager
        .systemctl(&["show", "--no-pager", &property, UNIT])
        .map_err(|error| match error.kind() {
            io::ErrorKind::TimedOut => ServiceFault::UnknownState,
            _ => ServiceFault::Manager,
        })
}

fn service_status_line(success: bool, output: &[u8]) -> Result<&'static str, ServiceFault> {
    let state = super::systemd_service_state(success, output);
    super::controller_service_status_line(state).ok_or(ServiceFault::UnknownState)
}

fn start(manager: &mut impl Manager, drain_timeout_secs: u64) -> Result<(), ServiceFault> {
    let before = read_snapshot(manager)?;
    verify_package_contract(&before, drain_timeout_secs)?;
    verify_load_credential(manager)?;
    verify_service_environment(manager)?;
    verify_identity_contract(&read_identity_snapshot(manager)?)?;
    if is_running(&before) {
        if before.result != "success" {
            return Err(ServiceFault::StartFailed);
        }
        return ensure_no_pending_unit_job(manager);
    }
    if !is_stopped(&before) {
        return Err(ServiceFault::UnknownState);
    }
    ensure_no_pending_unit_job(manager)?;
    if !manager_call(manager, &["start", UNIT])? {
        return Err(ServiceFault::StartFailed);
    }
    let after = read_snapshot(manager)?;
    verify_package_contract(&after, drain_timeout_secs)?;
    verify_load_credential(manager)?;
    verify_service_environment(manager)?;
    verify_identity_contract(&read_identity_snapshot(manager)?)?;
    if !is_running(&after) || after.result != "success" {
        return Err(ServiceFault::StartFailed);
    }
    ensure_no_pending_unit_job(manager)
}

fn read_identity_snapshot(
    manager: &mut impl Manager,
) -> Result<IdentityUnitSnapshot, ServiceFault> {
    let output = manager_call_output(
        manager,
        &[
            "show",
            "--no-pager",
            &format!("--property={IDENTITY_SHOW_PROPERTIES}"),
            IDENTITY_UNIT,
        ],
    )?;
    if !output.success {
        return Err(ServiceFault::IdentityUnitUnavailable);
    }
    let mut snapshot = parse_identity_snapshot(&output.stdout).ok_or(ServiceFault::UnknownState)?;
    snapshot.identity_marker_condition_matches =
        read_identity_marker_condition(manager, IDENTITY_OBJECT_PATH)?;
    Ok(snapshot)
}

fn verify_load_credential(manager: &mut impl Manager) -> Result<(), ServiceFault> {
    let output = manager
        .busctl(&[
            "--system",
            "get-property",
            SYSTEMD_BUS_NAME,
            UNIT_OBJECT_PATH,
            SERVICE_INTERFACE,
            CREDENTIAL_PROPERTY,
        ])
        .map_err(|_| ServiceFault::Manager)?;
    if !output.success {
        return Err(ServiceFault::CredentialUnavailable);
    }
    let value =
        std::str::from_utf8(&output.stdout).map_err(|_| ServiceFault::CredentialUnavailable)?;
    if value.trim() != EXPECTED_CREDENTIAL_PROPERTY {
        return Err(ServiceFault::CredentialUnavailable);
    }
    Ok(())
}

fn verify_service_environment(manager: &mut impl Manager) -> Result<(), ServiceFault> {
    let output = manager
        .busctl(&[
            "--system",
            "--timeout=5",
            "get-property",
            SYSTEMD_BUS_NAME,
            UNIT_OBJECT_PATH,
            SERVICE_INTERFACE,
            ENVIRONMENT_PROPERTY,
        ])
        .map_err(|_| ServiceFault::Manager)?;
    if !output.success {
        return Err(ServiceFault::Manager);
    }
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| ServiceFault::ServiceEnvironmentUnavailable)?;
    if value.trim() != EXPECTED_ENVIRONMENT_PROPERTY {
        return Err(ServiceFault::ServiceEnvironmentUnavailable);
    }
    Ok(())
}

fn read_snapshot(manager: &mut impl Manager) -> Result<UnitSnapshot, ServiceFault> {
    let mut snapshot = read_unit_snapshot(manager)?;
    snapshot.identity_marker_condition_matches =
        read_identity_marker_condition(manager, UNIT_OBJECT_PATH)?;
    Ok(snapshot)
}

fn read_unit_snapshot(manager: &mut impl Manager) -> Result<UnitSnapshot, ServiceFault> {
    let output = manager_call_output(
        manager,
        &[
            "show",
            "--no-pager",
            &format!("--property={SHOW_PROPERTIES}"),
            UNIT,
        ],
    )?;
    if !output.success {
        return Err(ServiceFault::UnitUnavailable);
    }
    parse_snapshot(&output.stdout).ok_or(ServiceFault::UnknownState)
}

fn is_running(snapshot: &UnitSnapshot) -> bool {
    snapshot.active_state == "active"
        && snapshot.sub_state == "running"
        && snapshot.main_pid > 0
        && snapshot.control_pid == 0
}

fn is_stopped(snapshot: &UnitSnapshot) -> bool {
    snapshot.active_state == "inactive"
        && snapshot.sub_state == "dead"
        && snapshot.main_pid == 0
        && snapshot.control_pid == 0
}

fn ensure_no_pending_unit_job(manager: &mut impl Manager) -> Result<(), ServiceFault> {
    let output = manager_call_output(manager, &["list-jobs", "--no-legend", "--no-pager"])?;
    if !output.success {
        return Err(ServiceFault::Manager);
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|_| ServiceFault::UnknownState)?;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 4 || fields[0].parse::<u64>().is_err() {
            return Err(ServiceFault::UnknownState);
        }
        if fields[1] == UNIT {
            return Err(ServiceFault::PendingJob);
        }
    }
    Ok(())
}

fn manager_call(manager: &mut impl Manager, args: &[&str]) -> Result<bool, ServiceFault> {
    manager_call_output(manager, args).map(|output| output.success)
}

fn manager_call_output(
    manager: &mut impl Manager,
    args: &[&str],
) -> Result<ManagerOutput, ServiceFault> {
    manager.systemctl(args).map_err(|error| {
        if error.kind() == io::ErrorKind::TimedOut {
            ServiceFault::DrainDeadline
        } else {
            ServiceFault::Manager
        }
    })
}

#[cfg(test)]
mod tests;
