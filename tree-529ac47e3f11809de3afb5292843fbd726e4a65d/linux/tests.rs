use std::collections::VecDeque;
use std::io;

use super::{
    ENVIRONMENT_PROPERTY, EXPECTED_CREDENTIAL_PROPERTY, EXPECTED_ENVIRONMENT_PROPERTY,
    IDENTITY_OBJECT_PATH, IDENTITY_UNIT, Manager, ManagerOutput, ServiceFault, StopTimeout, UNIT,
    conditions::condition_output, package_paths_supported, parse_stop_timeout, parse_timespan_usec,
    perform, preflight_argv, start_argv, stop_argv,
};
use crate::args::ServiceAction;

const DRAIN_TIMEOUT_SECS: u64 = 10;

#[derive(Default)]
struct FakeManager {
    outputs: VecDeque<ManagerOutput>,
    condition_outputs: VecDeque<ManagerOutput>,
    environment_outputs: VecDeque<ManagerOutput>,
    calls: Vec<Vec<String>>,
    busctl_calls: Vec<Vec<String>>,
}

impl FakeManager {
    fn with_outputs(outputs: impl IntoIterator<Item = ManagerOutput>) -> Self {
        Self {
            outputs: outputs.into_iter().collect(),
            condition_outputs: VecDeque::new(),
            environment_outputs: VecDeque::new(),
            calls: Vec::new(),
            busctl_calls: Vec::new(),
        }
    }

    fn with_condition_outputs(
        outputs: impl IntoIterator<Item = ManagerOutput>,
        condition_outputs: impl IntoIterator<Item = ManagerOutput>,
    ) -> Self {
        Self {
            outputs: outputs.into_iter().collect(),
            condition_outputs: condition_outputs.into_iter().collect(),
            environment_outputs: VecDeque::new(),
            calls: Vec::new(),
            busctl_calls: Vec::new(),
        }
    }
}

impl Manager for FakeManager {
    fn systemctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        self.calls
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        self.outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected systemctl invocation"))
    }

    fn busctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        self.busctl_calls
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        if args.last() == Some(&"Conditions") {
            return Ok(self
                .condition_outputs
                .pop_front()
                .unwrap_or_else(|| condition_output(true)));
        }
        if args.last() == Some(&ENVIRONMENT_PROPERTY) {
            return Ok(self
                .environment_outputs
                .pop_front()
                .unwrap_or_else(environment_property));
        }
        self.outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected busctl invocation"))
    }
}

fn manager_output(success: bool, stdout: impl Into<Vec<u8>>) -> ManagerOutput {
    ManagerOutput {
        success,
        stdout: stdout.into(),
    }
}

#[derive(Clone, Copy)]
struct UnitSnapshot<'a> {
    active_state: &'a str,
    sub_state: &'a str,
    main_pid: u32,
    control_pid: u32,
    result: &'a str,
    stop_code: &'a str,
    stop_status: &'a str,
    timeout: &'a str,
}

fn unit_snapshot(snapshot: UnitSnapshot<'_>) -> Vec<u8> {
    let UnitSnapshot {
        active_state,
        sub_state,
        main_pid,
        control_pid,
        result,
        stop_code,
        stop_status,
        timeout,
    } = snapshot;
    let start = start_argv().join(" ");
    let preflight = preflight_argv().join(" ");
    let stop = stop_argv().join(" ");
    let identity_unit = IDENTITY_UNIT;
    format!(
        "Id=velnor-host.service\nLoadState=loaded\nActiveState={active_state}\nSubState={sub_state}\nMainPID={main_pid}\nControlPID={control_pid}\nResult={result}\nJob=\nExecStartPre={{ path=/usr/bin/velnor-host ; argv[]={preflight} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nExecStart={{ path=/usr/bin/velnor-host ; argv[]={start} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid={main_pid} ; code=(null) ; status=0/0 }}\nExecStop={{ path=/usr/bin/velnor-host ; argv[]={stop} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code={stop_code} ; status={stop_status} }}\nTimeoutStopUSec={timeout}\nTimeoutStopFailureMode=terminate\nKillSignal=15\nKillMode=mixed\nUser=velnor\nGroup=velnor\nSupplementaryGroups=docker\nWorkingDirectory=/var/lib/velnor-host\nUMask=0077\nNoNewPrivileges=yes\nProtectSystem=strict\nReadWritePaths=/var/lib/velnor-host\nRequires=docker.service {identity_unit}\nAfter=network-online.target docker.service {identity_unit}\nType=simple\n"
    )
    .into_bytes()
}

fn identity_unit_snapshot() -> ManagerOutput {
    manager_output(
        true,
        b"LoadState=loaded\nExecStart={ path=/usr/lib/velnor-host/account-check ; argv[]=/usr/lib/velnor-host/account-check --verify ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\nUser=root\nGroup=root\nUMask=0077\nNoNewPrivileges=yes\nProtectSystem=strict\nType=oneshot\nRemainAfterExit=no\nBefore=velnor-host.service\n".to_vec(),
    )
}

fn credential_property() -> ManagerOutput {
    manager_output(
        true,
        format!("{EXPECTED_CREDENTIAL_PROPERTY}\n").into_bytes(),
    )
}

fn environment_property() -> ManagerOutput {
    manager_output(
        true,
        format!("{EXPECTED_ENVIRONMENT_PROPERTY}\n").into_bytes(),
    )
}

fn empty_jobs() -> ManagerOutput {
    manager_output(true, Vec::new())
}

#[test]
fn status_reads_only_systemd_state_and_fails_closed_on_unknown() {
    let stopped =
        b"LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nControlPID=0\n";
    let mut manager = FakeManager::with_outputs([manager_output(true, stopped.to_vec())]);
    assert_eq!(perform(ServiceAction::Status, &mut manager, 0), Ok(()));
    assert_eq!(
        manager.calls,
        vec![vec![
            "show".to_owned(),
            "--no-pager".to_owned(),
            "--property=LoadState,ActiveState,SubState,MainPID,ControlPID".to_owned(),
            UNIT.to_owned(),
        ]]
    );
    assert_eq!(manager.busctl_calls, Vec::<Vec<String>>::new());

    let mut manager = FakeManager::with_outputs([manager_output(false, stopped.to_vec())]);
    assert_eq!(
        perform(ServiceAction::Status, &mut manager, 0),
        Err(ServiceFault::UnknownState)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_does_not_mutate_a_unit_with_an_unexpected_command() {
    let snapshot = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let altered = String::from_utf8_lossy(&snapshot)
        .replace(
            "--config /etc/velnor-host/host.toml",
            "--config /tmp/other.toml",
        )
        .into_bytes();
    let mut manager = FakeManager::with_outputs([manager_output(true, altered)]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_requires_the_root_identity_preflight_dependency_and_order() {
    let valid = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let altered = String::from_utf8_lossy(&valid).replace(&format!(" {IDENTITY_UNIT}"), "");
    let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_rejects_writable_host_paths_outside_state_directory() {
    let valid = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let altered = String::from_utf8_lossy(&valid).replace(
        "ReadWritePaths=/var/lib/velnor-host",
        "ReadWritePaths=/var/lib/velnor-host /etc",
    );
    let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_rejects_an_unbounded_stop_timeout_before_mutating_the_unit() {
    let snapshot = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "infinity",
    });
    let mut manager = FakeManager::with_outputs([manager_output(true, snapshot)]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_requires_stop_timeout_to_exceed_the_configured_drain_timeout() {
    let equal = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "10s",
    });
    let mut manager = FakeManager::with_outputs([manager_output(true, equal)]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn low_level_stop_helper_fails_closed_without_the_coordinator() {
    let active = unit_snapshot(UnitSnapshot {
        active_state: "active",
        sub_state: "running",
        main_pid: 42,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let mut manager = FakeManager::with_outputs([manager_output(true, active)]);

    assert_eq!(
        perform(ServiceAction::Stop, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::StopCoordinatorRequired)
    );
    assert_eq!(manager.calls, Vec::<Vec<String>>::new());
}

#[test]
fn install_and_uninstall_are_owned_by_the_package() {
    let mut manager = FakeManager::default();
    assert_eq!(
        perform(ServiceAction::Install, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::PackageOwned)
    );
    assert_eq!(
        perform(ServiceAction::Uninstall, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::PackageOwned)
    );
    assert_eq!(manager.calls, Vec::<Vec<String>>::new());
}

#[test]
fn service_path_overrides_are_rejected_instead_of_ignored() {
    assert!(package_paths_supported(
        std::path::Path::new(CONFIG_PATH),
        std::path::Path::new(STATE_PATH)
    ));
    assert!(!package_paths_supported(
        std::path::Path::new("/tmp/host.toml"),
        std::path::Path::new(STATE_PATH)
    ));
    assert!(!package_paths_supported(
        std::path::Path::new(CONFIG_PATH),
        std::path::Path::new("/tmp/state")
    ));
}

#[test]
fn stop_timeout_parser_accepts_systemd_finite_timespans_and_rejects_unbounded_or_malformed() {
    assert_eq!(parse_timespan_usec("30s"), Some(30_000_000));
    assert_eq!(parse_timespan_usec("1min 250ms"), Some(60_250_000));
    assert_eq!(
        parse_stop_timeout("30s"),
        Some(StopTimeout::Finite(30_000_000))
    );
    assert_eq!(parse_stop_timeout("infinity"), Some(StopTimeout::Infinite));
    assert_eq!(parse_timespan_usec("0"), None);
    assert_eq!(parse_timespan_usec("999999999999999999999999999999h"), None);
    assert_eq!(parse_timespan_usec("30fortnights"), None);
}

const CONFIG_PATH: &str = "/etc/velnor-host/host.toml";
const STATE_PATH: &str = "/var/lib/velnor-host";

mod contract_tests;
mod credential_tests;

mod environment_tests;
mod identity_tests;
mod start_contract_tests;
mod stop_tests;
mod verify_stopped_tests;
