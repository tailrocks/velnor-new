//! systemd lifecycle commands for the package-owned controller unit.

use std::io;
use std::path::Path;
use std::process::{Command, ExitCode};

use velnor_runner_host::{HostConfig, HostPlatform};

use crate::args::ServiceAction;
use conditions::read_identity_marker_condition;
use systemd::{
    IdentityUnitSnapshot, StopTimeout, UnitSnapshot, parse_identity_snapshot, parse_snapshot,
};
#[cfg(test)]
use systemd::{parse_stop_timeout, parse_timespan_usec};

mod conditions;
mod systemd;

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
const SHOW_PROPERTIES: &str = concat!(
    "LoadState,ActiveState,SubState,MainPID,ControlPID,Result,ExecStart,ExecStop,",
    "TimeoutStopUSec,User,Group,SupplementaryGroups,WorkingDirectory,UMask,",
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

struct Systemctl;

impl Manager for Systemctl {
    fn systemctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        let output = Command::new("systemctl").args(args).output()?;
        Ok(ManagerOutput {
            success: output.status.success(),
            stdout: output.stdout,
        })
    }

    fn busctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        let output = Command::new("busctl").args(args).output()?;
        Ok(ManagerOutput {
            success: output.status.success(),
            stdout: output.stdout,
        })
    }
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
    DrainUnavailable,
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
            Self::DrainUnavailable => {
                "safe service stop is unavailable until drain reconciliation is implemented"
            }
            Self::PendingJob => "velnor-host.service still has a pending systemd job",
            Self::PackageOwned => "install and uninstall are managed by the system package",
            Self::InvalidConfig => "Linux host configuration is missing or invalid",
        }
    }
}

pub(super) fn service(action: ServiceAction, config_path: &Path, state_path: &Path) -> ExitCode {
    if matches!(action, ServiceAction::Install | ServiceAction::Uninstall) {
        eprintln!("{}", ServiceFault::PackageOwned.message());
        return ExitCode::from(1);
    }
    if !package_paths_supported(config_path, state_path) {
        eprintln!("{}", ServiceFault::InvalidConfig.message());
        return ExitCode::from(1);
    }
    let drain_timeout_secs = match configured_drain_timeout(config_path) {
        Ok(timeout) => timeout,
        Err(fault) => {
            eprintln!("{}", fault.message());
            return ExitCode::from(1);
        }
    };
    let mut manager = Systemctl;
    match perform(action, &mut manager, drain_timeout_secs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("{}", fault.message());
            ExitCode::from(1)
        }
    }
}

fn perform(
    action: ServiceAction,
    manager: &mut impl Manager,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    match action {
        ServiceAction::Install | ServiceAction::Uninstall => Err(ServiceFault::PackageOwned),
        ServiceAction::Start => start(manager, drain_timeout_secs),
        ServiceAction::Stop => Err(ServiceFault::DrainUnavailable),
    }
}

fn start(manager: &mut impl Manager, drain_timeout_secs: u64) -> Result<(), ServiceFault> {
    let before = read_snapshot(manager)?;
    verify_package_contract(&before, drain_timeout_secs)?;
    verify_load_credential(manager)?;
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

fn read_snapshot(manager: &mut impl Manager) -> Result<UnitSnapshot, ServiceFault> {
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
    let mut snapshot = parse_snapshot(&output.stdout).ok_or(ServiceFault::UnknownState)?;
    snapshot.identity_marker_condition_matches =
        read_identity_marker_condition(manager, UNIT_OBJECT_PATH)?;
    Ok(snapshot)
}

fn verify_package_contract(
    snapshot: &UnitSnapshot,
    drain_timeout_secs: u64,
) -> Result<(), ServiceFault> {
    let drain_timeout_usec = u128::from(drain_timeout_secs)
        .checked_mul(1_000_000)
        .ok_or(ServiceFault::InvalidConfig)?;
    if snapshot.load_state != "loaded"
        || snapshot.exec_start.path != BINARY
        || snapshot.exec_start.argv != start_argv()
        || snapshot.exec_start.ignore_errors != "no"
        || snapshot.exec_stop.path != BINARY
        || snapshot.exec_stop.argv != stop_argv()
        || snapshot.exec_stop.ignore_errors != "no"
        || !matches!(
            snapshot.timeout_stop,
            StopTimeout::Finite(timeout) if timeout > drain_timeout_usec
        )
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
        || snapshot.unit_type != "exec"
        || !snapshot.identity_marker_condition_matches
    {
        return Err(ServiceFault::ServiceContract);
    }
    Ok(())
}

fn verify_identity_contract(snapshot: &IdentityUnitSnapshot) -> Result<(), ServiceFault> {
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

fn configured_drain_timeout(config_path: &Path) -> Result<u64, ServiceFault> {
    let text = std::fs::read_to_string(config_path).map_err(|_| ServiceFault::InvalidConfig)?;
    let config = HostConfig::parse(&text).map_err(|_| ServiceFault::InvalidConfig)?;
    config
        .validate_for_host(HostPlatform::Linux)
        .map_err(|_| ServiceFault::InvalidConfig)?;
    config
        .drain_timeout_secs()
        .map_err(|_| ServiceFault::InvalidConfig)
}

fn package_paths_supported(config_path: &Path, state_path: &Path) -> bool {
    config_path == Path::new(CONFIG) && state_path == Path::new(STATE)
}

fn start_argv() -> Vec<String> {
    [
        BINARY, "--config", CONFIG, "--state", STATE, "daemon", "run",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn stop_argv() -> Vec<String> {
    [
        BINARY, "--config", CONFIG, "--state", STATE, "drain", "--wait",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
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
    manager.systemctl(args).map_err(|_| ServiceFault::Manager)
}

#[cfg(test)]
mod tests;
