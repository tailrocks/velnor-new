use super::*;

#[test]
fn parse_execstartpre_matches_systemctl_output_from_loaded_unit() {
    // Read-only observation from systemd 257.13's loaded ssh.service. The
    // parser must retain argv boundaries and the systemd error policy used by
    // the package's boot-time ExecStartPre contract.
    let mut service = String::from_utf8(unit_snapshot(UnitSnapshot {
        active_state: "activating",
        sub_state: "start-pre",
        main_pid: 0,
        control_pid: 1,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "31s",
    }))
    .expect("ASCII unit fixture");
    let package_preflight = format!(
        "ExecStartPre={{ path=/usr/bin/velnor-host ; argv[]={} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}",
        preflight_argv().join(" ")
    );
    let observed = "ExecStartPre={ path=/usr/sbin/sshd ; argv[]=/usr/sbin/sshd -t ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }";
    assert!(service.contains(&package_preflight));
    service = service.replace(&package_preflight, observed);

    let parsed = super::super::systemd::parse_snapshot(service.as_bytes())
        .expect("systemctl show output from a loaded unit");
    let invocation = parsed.exec_start_pre;

    assert_eq!(invocation.path, "/usr/sbin/sshd");
    assert_eq!(invocation.argv, ["/usr/sbin/sshd", "-t"]);
    assert_eq!(invocation.ignore_errors, "no");
}

#[test]
fn start_rejects_commands_that_ignore_failures_or_have_incomplete_error_policy() {
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
    let source = String::from_utf8_lossy(&valid);
    for command in ["ExecStartPre=", "ExecStart=", "ExecStop="] {
        let altered = source
            .lines()
            .map(|line| {
                if line.starts_with(command) {
                    line.replacen("ignore_errors=no", "ignore_errors=yes", 1)
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);
        assert_eq!(
            perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
            Err(ServiceFault::ServiceContract)
        );
        assert_eq!(manager.calls.len(), 1);
        assert_eq!(manager.busctl_calls.len(), 1);
        assert_eq!(
            manager.busctl_calls[0].last().map(String::as_str),
            Some("Conditions")
        );
    }

    for malformed in [
        source.replacen("ignore_errors=no", "", 1),
        source.replacen("ignore_errors=no", "ignore_errors=no ; ignore_errors=no", 1),
    ] {
        let mut manager = FakeManager::with_outputs([manager_output(true, malformed.into_bytes())]);
        assert_eq!(
            perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
            Err(ServiceFault::UnknownState)
        );
        assert_eq!(manager.calls.len(), 1);
        assert_eq!(manager.busctl_calls, Vec::<Vec<String>>::new());
    }
}

#[test]
fn start_rejects_waiting_exec_stop_command() {
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
    let source = String::from_utf8_lossy(&valid);
    let stale = source.replacen(" drain ;", " drain --wait ;", 1);
    assert_ne!(stale, source);

    let mut manager = FakeManager::with_outputs([manager_output(true, stale.into_bytes())]);
    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
    assert_eq!(manager.busctl_calls.len(), 1);
    assert_eq!(
        manager.busctl_calls[0].last().map(String::as_str),
        Some("Conditions")
    );
}

#[test]
fn start_rejects_exec_stop_timeout_mode_that_skips_sigterm() {
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
    let source = String::from_utf8_lossy(&valid);
    let altered = source.replace(
        "TimeoutStopFailureMode=terminate",
        "TimeoutStopFailureMode=abort",
    );
    assert_ne!(altered, source);

    let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);
    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_rejects_exec_stop_kill_signal_that_skips_sigterm() {
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
    let source = String::from_utf8_lossy(&valid);
    let altered = source.replace("KillSignal=15", "KillSignal=9");
    assert_ne!(altered, source);

    let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);
    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::ServiceContract)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_rejects_kill_modes_that_can_suppress_or_broaden_signal_delivery() {
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
    let source = String::from_utf8_lossy(&valid);
    for kill_mode in ["none", "control-group", "process"] {
        let altered = source.replace("KillMode=mixed", &format!("KillMode={kill_mode}"));
        assert_ne!(altered, source);
        let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);
        assert_eq!(
            perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
            Err(ServiceFault::ServiceContract)
        );
        assert_eq!(manager.calls.len(), 1);
    }

    let missing = source.replace("KillMode=mixed\n", "");
    let mut manager = FakeManager::with_outputs([manager_output(true, missing.into_bytes())]);
    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Err(ServiceFault::UnknownState)
    );
    assert_eq!(manager.calls.len(), 1);
}

#[test]
fn start_requires_the_controller_identity_marker_condition() {
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
    for condition in [
        manager_output(true, b"a(sbbsi) 0\n".to_vec()),
        manager_output(
            true,
            b"a(sbbsi) 1 \"ConditionPathExists\" false false \"/tmp/other-marker\" 0\n".to_vec(),
        ),
        manager_output(
            true,
            b"a(sbbsi) 1 \"ConditionPathExists\" true false \"/var/lib/velnor-host-package/identity\" 0\n".to_vec(),
        ),
        manager_output(
            true,
            b"a(sbbsi) 1 \"ConditionPathExists\" false true \"/var/lib/velnor-host-package/identity\" 0\n".to_vec(),
        ),
        manager_output(
            true,
            b"a(sbbsi) 2 \"ConditionPathExists\" false false \"/var/lib/velnor-host-package/identity\" 0 \"ConditionPathExists\" false false \"/var/lib/velnor-host-package/identity\" 0\n".to_vec(),
        ),
        manager_output(
            true,
            b"a(ss) 1 \"ConditionPathExists\" \"/var/lib/velnor-host-package/identity\"\n".to_vec(),
        ),
        manager_output(
            true,
            b"a(sbbsi) 1 \"ConditionPathExists\" false false \"/var/lib/velnor-host-package/identity\" 2\n".to_vec(),
        ),
        manager_output(false, Vec::new()),
    ] {
        let mut manager = FakeManager::with_condition_outputs(
            [manager_output(true, valid.clone())],
            [condition],
        );
        assert_ne!(
            perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
            Ok(())
        );
        assert_eq!(manager.calls.len(), 1);
        assert_eq!(manager.busctl_calls.len(), 1);
    }
}
