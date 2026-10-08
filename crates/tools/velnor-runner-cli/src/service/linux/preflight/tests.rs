use std::collections::VecDeque;
use std::io;
use std::path::Path;
use velnor_runner_host::{HostError, HostPlatform};

use super::super::{
    ENVIRONMENT_PROPERTY, EXPECTED_CREDENTIAL_PROPERTY, EXPECTED_ENVIRONMENT_PROPERTY,
    IDENTITY_UNIT, Manager, ManagerOutput, ServiceFault, UNIT, conditions::condition_output,
    perform,
};
use super::{verify_loaded_unit, verify_loaded_unit_for_config};
use crate::args::ServiceAction;
use velnor_runner_host::MAX_LINUX_DRAIN_TIMEOUT_SECS;

#[derive(Default)]
struct FakeManager {
    systemctl_outputs: VecDeque<ManagerOutput>,
    busctl_outputs: VecDeque<ManagerOutput>,
    environment_outputs: VecDeque<ManagerOutput>,
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
        if args.last() == Some(&ENVIRONMENT_PROPERTY) {
            return Ok(self
                .environment_outputs
                .pop_front()
                .unwrap_or_else(environment_property));
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

fn environment_property() -> ManagerOutput {
    output(
        true,
        format!("{EXPECTED_ENVIRONMENT_PROPERTY}\n").into_bytes(),
    )
}

fn loaded_service(timeout: &str) -> ManagerOutput {
    let start = "/usr/bin/velnor-host --config /etc/velnor-host/host.toml --state /var/lib/velnor-host daemon run";
    let preflight = "/usr/bin/velnor-host --config /etc/velnor-host/host.toml --state /var/lib/velnor-host service preflight";
    let stop = "/usr/bin/velnor-host --config /etc/velnor-host/host.toml --state /var/lib/velnor-host drain";
    output(
        true,
        format!(
            "LoadState=loaded\nActiveState=activating\nSubState=start-pre\nMainPID=0\nControlPID=71\nResult=success\nExecStartPre={{ path=/usr/bin/velnor-host ; argv[]={preflight} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nExecStart={{ path=/usr/bin/velnor-host ; argv[]={start} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nExecStop={{ path=/usr/bin/velnor-host ; argv[]={stop} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\nTimeoutStopUSec={timeout}\nTimeoutStopFailureMode=terminate\nKillSignal=15\nKillMode=mixed\nUser=velnor\nGroup=velnor\nSupplementaryGroups=docker\nWorkingDirectory=/var/lib/velnor-host\nUMask=0077\nNoNewPrivileges=yes\nProtectSystem=strict\nReadWritePaths=/var/lib/velnor-host\nRequires=docker.service velnor-host-identity-check.service\nAfter=network-online.target docker.service velnor-host-identity-check.service\nType=simple\n"
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
fn preflight_accepts_activation_state_with_the_pinned_finite_package_stop_budget() {
    let mut manager = valid_manager("1860s");

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
    assert_eq!(manager.busctl_calls.len(), 4);
    assert!(manager.busctl_calls.iter().any(|call| {
        call.get(4)
            .is_some_and(|path| path == "/org/freedesktop/systemd1/unit/velnor_2dhost_2eservice")
            && call
                .get(5)
                .is_some_and(|interface| interface == "org.freedesktop.systemd1.Service")
            && call
                .last()
                .is_some_and(|property| property == ENVIRONMENT_PROPERTY)
    }));
    assert!(
        manager
            .busctl_calls
            .iter()
            .any(|call| call.last().is_some_and(|v| v == "LoadCredential"))
    );
}

#[test]
fn preflight_rejects_missing_redirected_or_non_singleton_environment_before_identity_query() {
    for environment in [
        b"as 0\n".as_slice(),
        b"as 1 \"PATH=/usr/bin:/bin\"\n".as_slice(),
        b"as 2 \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\" \"EXTRA=value\"\n".as_slice(),
        b"as 1 \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\" \"EXTRA=value\"\n".as_slice(),
        b"s \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\"\n".as_slice(),
    ] {
        let mut manager = valid_manager("1860s");
        manager
            .environment_outputs
            .push_back(output(true, environment.to_vec()));

        assert_eq!(
            verify_loaded_unit(&mut manager, 30),
            Err(ServiceFault::ServiceEnvironmentUnavailable)
        );
        assert_eq!(manager.systemctl_calls.len(), 1);
        assert_eq!(manager.busctl_calls.len(), 3);
        assert!(manager.busctl_calls.last().is_some_and(|call| {
            call.last()
                .is_some_and(|property| property == ENVIRONMENT_PROPERTY)
        }));
    }
}

#[test]
fn preflight_rejects_unpinned_stop_timeout() {
    let mut manager = valid_manager("1859s");

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
fn preflight_rejects_out_of_range_configured_drain_without_contacting_systemd() {
    for timeout in [0, MAX_LINUX_DRAIN_TIMEOUT_SECS + 1] {
        let mut manager = FakeManager::default();

        assert_eq!(
            verify_loaded_unit(&mut manager, timeout),
            Err(ServiceFault::InvalidConfig),
            "unexpected acceptance for drain timeout {timeout}"
        );
        assert_eq!(manager.systemctl_calls.len(), 0);
        assert_eq!(manager.busctl_calls.len(), 0);
    }
}

#[test]
fn preflight_fails_closed_when_the_loaded_credential_mapping_differs() {
    let mut manager = valid_manager("1860s");
    manager.busctl_outputs[0] = output(true, b"a(ss) 1 \"other\" \"/tmp/credential\"\n".to_vec());

    assert_eq!(
        verify_loaded_unit(&mut manager, 30),
        Err(ServiceFault::CredentialUnavailable)
    );
    assert_eq!(manager.systemctl_calls.len(), 1);
}

#[test]
fn preflight_rejects_a_missing_or_redirected_execstartpre_before_credentials() {
    let source = String::from_utf8_lossy(&loaded_service("1860s").stdout).into_owned();
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
    let mut manager = valid_manager("1860s");

    assert_eq!(perform(ServiceAction::Preflight, &mut manager, 30), Ok(()));
    assert_eq!(manager.systemctl_calls.len(), 2);
    assert_eq!(manager.busctl_calls.len(), 4);
}

#[test]
fn protected_config_reader_failures_do_not_contact_systemd() {
    // Owner/mode, symlink, and regular-file checks belong to the protected
    // host reader. Inject its rejection result here to prove the preflight
    // command stops before any manager request for each rejected file class.
    for failure in ["insecure mode", "symlink", "fifo"] {
        let mut manager = FakeManager::default();
        let result = verify_loaded_unit_for_config(
            &mut manager,
            Path::new("/etc/velnor-host/host.toml"),
            move |path, platform| {
                assert_eq!(path, Path::new("/etc/velnor-host/host.toml"));
                assert_eq!(platform, HostPlatform::Linux);
                match failure {
                    "insecure mode" | "symlink" | "fifo" => Err(HostError::Config),
                    _ => unreachable!("test case is fixed"),
                }
            },
        );

        assert_eq!(result, Err(ServiceFault::InvalidConfig), "{failure}");
        assert_eq!(
            manager.systemctl_calls,
            Vec::<Vec<String>>::new(),
            "{failure}"
        );
        assert_eq!(manager.busctl_calls, Vec::<Vec<String>>::new(), "{failure}");
    }
}

#[test]
fn protected_config_reader_requires_a_present_valid_linux_file_before_manager_queries() {
    let mut manager = FakeManager::default();
    let missing = verify_loaded_unit_for_config(
        &mut manager,
        Path::new("/etc/velnor-host/host.toml"),
        |_, platform| {
            assert_eq!(platform, HostPlatform::Linux);
            Ok(None)
        },
    );
    assert_eq!(missing, Err(ServiceFault::InvalidConfig));
    assert_eq!(manager.systemctl_calls, Vec::<Vec<String>>::new());

    let mut manager = FakeManager::default();
    let invalid = verify_loaded_unit_for_config(
        &mut manager,
        Path::new("/etc/velnor-host/host.toml"),
        |_, platform| {
            assert_eq!(platform, HostPlatform::Linux);
            Ok(Some("schema = 2\n".to_owned()))
        },
    );
    assert_eq!(invalid, Err(ServiceFault::InvalidConfig));
    assert_eq!(manager.systemctl_calls, Vec::<Vec<String>>::new());
}
