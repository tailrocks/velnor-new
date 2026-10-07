use std::collections::VecDeque;
use std::io;

use super::super::{
    EXPECTED_CREDENTIAL_PROPERTY, IDENTITY_UNIT, Manager, ManagerOutput, ServiceFault, UNIT,
    conditions::condition_output, perform,
};
use super::verify_loaded_unit;
use crate::args::ServiceAction;

#[derive(Default)]
struct FakeManager {
    systemctl_outputs: VecDeque<ManagerOutput>,
    busctl_outputs: VecDeque<ManagerOutput>,
    systemctl_calls: Vec<Vec<String>>,
    busctl_calls: Vec<Vec<String>>,
}

impl Manager for FakeManager {
    fn systemctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        self.systemctl_calls
            .push(args.iter().map(|value| (*value).to_owned()).collect());
        self.systemctl_outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected systemctl call"))
    }

    fn busctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        self.busctl_calls
            .push(args.iter().map(|value| (*value).to_owned()).collect());
        if args.last() == Some(&"Conditions") {
            return Ok(condition_output(true));
        }
        self.busctl_outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected busctl call"))
    }
}

fn output(success: bool, stdout: impl Into<Vec<u8>>) -> ManagerOutput {
    ManagerOutput {
        success,
        stdout: stdout.into(),
    }
}

fn loaded_service(timeout: &str) -> ManagerOutput {
    let start = "/usr/bin/velnor-host --config /etc/velnor-host/host.toml --state /var/lib/velnor-host daemon run";
    let preflight = "/usr/bin/velnor-host --config /etc/velnor-host/host.toml --state /var/lib/velnor-host service preflight";
    let stop = "/usr/bin/velnor-host --config /etc/velnor-host/host.toml --state /var/lib/velnor-host drain --wait";
    output(
        true,
        format!(
            "LoadState=loaded\nActiveState=activating\nSubState=start-pre\nMainPID=0\nControlPID=71\nResult=success\nExecStartPre={{ path=/usr/bin/velnor-host ; argv[]={preflight} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nExecStart={{ path=/usr/bin/velnor-host ; argv[]={start} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nExecStop={{ path=/usr/bin/velnor-host ; argv[]={stop} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nTimeoutStopUSec={timeout}\nUser=velnor\nGroup=velnor\nSupplementaryGroups=docker\nWorkingDirectory=/var/lib/velnor-host\nUMask=0077\nNoNewPrivileges=yes\nProtectSystem=strict\nReadWritePaths=/var/lib/velnor-host\nRequires=docker.service velnor-host-identity-check.service\nAfter=network-online.target docker.service velnor-host-identity-check.service\nType=simple\n"
        )
        .into_bytes(),
    )
}

fn identity_service() -> ManagerOutput {
    output(
        true,
        b"LoadState=loaded\nExecStart={ path=/usr/lib/velnor-host/account-check ; argv[]=/usr/lib/velnor-host/account-check --verify ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\nUser=root\nGroup=root\nUMask=0077\nNoNewPrivileges=yes\nProtectSystem=strict\nType=oneshot\nRemainAfterExit=no\nBefore=velnor-host.service\n".to_vec(),
    )
}

fn valid_manager(timeout: &str) -> FakeManager {
    FakeManager {
        systemctl_outputs: [loaded_service(timeout), identity_service()].into(),
        busctl_outputs: [output(
            true,
            format!("{EXPECTED_CREDENTIAL_PROPERTY}\n").into_bytes(),
        )]
        .into(),
        ..FakeManager::default()
    }
}

#[test]
fn preflight_accepts_activation_state_when_effective_stop_timeout_exceeds_configured_drain() {
    let mut manager = valid_manager("31s");

    assert_eq!(verify_loaded_unit(&mut manager, 30), Ok(()));
    assert_eq!(manager.systemctl_calls.len(), 2);
    assert_eq!(
        manager.systemctl_calls[0].last().map(String::as_str),
        Some(UNIT)
    );
    assert!(manager.systemctl_calls[0][2].contains("ExecStartPre"));
    assert_eq!(
        manager.systemctl_calls[1].last().map(String::as_str),
        Some(IDENTITY_UNIT)
    );
    assert_eq!(manager.busctl_calls.len(), 3);
    assert!(
        manager
            .busctl_calls
            .iter()
            .any(|call| call.last().is_some_and(|v| v == "LoadCredential"))
    );
}

#[test]
fn preflight_rejects_equal_stop_timeout() {
    let mut manager = valid_manager("30s");

    assert_eq!(
        verify_loaded_unit(&mut manager, 30),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.systemctl_calls.len(), 1);
    assert_eq!(manager.busctl_calls.len(), 1);
    assert!(
        !manager
            .busctl_calls
            .iter()
            .any(|call| call.last().is_some_and(|value| value == "LoadCredential"))
    );
}

#[test]
fn preflight_rejects_shorter_stop_timeout() {
    let mut manager = valid_manager("29s");

    assert_eq!(
        verify_loaded_unit(&mut manager, 30),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.systemctl_calls.len(), 1);
    assert_eq!(manager.busctl_calls.len(), 1);
    assert!(
        !manager
            .busctl_calls
            .iter()
            .any(|call| call.last().is_some_and(|value| value == "LoadCredential"))
    );
}

#[test]
fn preflight_rejects_infinite_stop_timeout() {
    let mut manager = valid_manager("infinity");

    assert_eq!(
        verify_loaded_unit(&mut manager, 30),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.systemctl_calls.len(), 1);
    assert_eq!(manager.busctl_calls.len(), 1);
    assert!(
        !manager
            .busctl_calls
            .iter()
            .any(|call| call.last().is_some_and(|value| value == "LoadCredential"))
    );
}

#[test]
fn preflight_rejects_zero_configured_drain_without_contacting_systemd() {
    let mut manager = FakeManager::default();

    assert_eq!(
        verify_loaded_unit(&mut manager, 0),
        Err(ServiceFault::InvalidConfig)
    );
    assert_eq!(manager.systemctl_calls.len(), 0);
    assert_eq!(manager.busctl_calls.len(), 0);
}

#[test]
fn preflight_fails_closed_when_the_loaded_credential_mapping_differs() {
    let mut manager = valid_manager("31s");
    manager.busctl_outputs[0] = output(true, b"a(ss) 1 \"other\" \"/tmp/credential\"\n".to_vec());

    assert_eq!(
        verify_loaded_unit(&mut manager, 30),
        Err(ServiceFault::CredentialUnavailable)
    );
    assert_eq!(manager.systemctl_calls.len(), 1);
}

#[test]
fn preflight_rejects_a_missing_or_redirected_execstartpre_before_credentials() {
    let source = String::from_utf8_lossy(&loaded_service("31s").stdout).into_owned();
    let missing = source.replace("ExecStartPre=", "ExecStartPreBackup=");
    let mut manager = FakeManager {
        systemctl_outputs: [output(true, missing.into_bytes())].into(),
        ..FakeManager::default()
    };
    assert_eq!(
        verify_loaded_unit(&mut manager, 30),
        Err(ServiceFault::UnknownState)
    );
    assert_eq!(manager.systemctl_calls.len(), 1);
    assert_eq!(manager.busctl_calls, Vec::<Vec<String>>::new());

    for malformed in [
        source.replace("service preflight", "daemon run"),
        source.replace("ignore_errors=no", "ignore_errors=yes"),
    ] {
        let mut manager = FakeManager {
            systemctl_outputs: [output(true, malformed.into_bytes())].into(),
            ..FakeManager::default()
        };
        assert_eq!(
            verify_loaded_unit(&mut manager, 30),
            Err(ServiceFault::ServiceContract)
        );
        assert_eq!(manager.systemctl_calls.len(), 1);
        assert_eq!(manager.busctl_calls.len(), 1);
    }
}

#[test]
fn service_preflight_dispatch_accepts_the_systemd_activation_phase() {
    let mut manager = valid_manager("31s");

    assert_eq!(perform(ServiceAction::Preflight, &mut manager, 30), Ok(()));
    assert_eq!(manager.systemctl_calls.len(), 2);
    assert_eq!(manager.busctl_calls.len(), 3);
}
