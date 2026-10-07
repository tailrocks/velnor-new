use super::*;

#[test]
fn start_requires_the_root_oneshot_identity_preflight_unit() {
    let stopped = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let mut manager = FakeManager::with_outputs([
        manager_output(true, stopped),
        credential_property(),
        manager_output(
            true,
            b"LoadState=loaded\nExecStart={ path=/usr/lib/velnor-host/account-check ; argv[]=/usr/lib/velnor-host/account-check --verify ; ignore_errors=no }\nUser=velnor\nGroup=root\nUMask=0077\nNoNewPrivileges=yes\nProtectSystem=strict\nType=oneshot\nRemainAfterExit=no\nBefore=velnor-host.service\n".to_vec(),
        ),
    ]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 2);
}

#[test]
fn start_rejects_an_identity_check_whose_failure_systemd_would_ignore() {
    let stopped = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let ignored_failure = String::from_utf8_lossy(&identity_unit_snapshot().stdout)
        .replace("ignore_errors=no", "ignore_errors=yes")
        .into_bytes();
    let mut manager = FakeManager::with_outputs([
        manager_output(true, stopped),
        credential_property(),
        manager_output(true, ignored_failure),
    ]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 2);
    assert!(
        !manager
            .calls
            .iter()
            .any(|call| call.first().is_some_and(|verb| verb == "start"))
    );
    assert_eq!(manager.busctl_calls.len(), 3);
}

#[test]
fn start_requires_identity_preflight_to_run_before_the_controller() {
    let stopped = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "30s",
    });
    let valid_identity = identity_unit_snapshot();
    let unordered_identity = String::from_utf8_lossy(&valid_identity.stdout)
        .replace("Before=velnor-host.service\n", "Before=\n")
        .into_bytes();
    let mut manager = FakeManager::with_outputs([
        manager_output(true, stopped),
        credential_property(),
        manager_output(true, unordered_identity),
    ]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 2);
    assert!(
        !manager
            .calls
            .iter()
            .any(|call| call.first().is_some_and(|verb| verb == "start"))
    );
    assert_eq!(manager.busctl_calls.len(), 3);
}
