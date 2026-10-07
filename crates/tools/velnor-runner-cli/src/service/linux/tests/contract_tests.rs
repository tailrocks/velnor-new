use super::*;

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
    for command in ["ExecStart=", "ExecStop="] {
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
