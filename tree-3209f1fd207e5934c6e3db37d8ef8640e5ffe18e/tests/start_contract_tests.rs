use super::*;

#[test]
fn start_verifies_the_packaged_unit_and_running_postcondition() {
    let stopped = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    });
    let active = unit_snapshot(UnitSnapshot {
        active_state: "active",
        sub_state: "running",
        main_pid: 42,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    });
    let mut manager = FakeManager::with_outputs([
        manager_output(true, stopped),
        credential_property(),
        identity_unit_snapshot(),
        empty_jobs(),
        manager_output(true, Vec::new()),
        manager_output(true, active),
        credential_property(),
        identity_unit_snapshot(),
        empty_jobs(),
    ]);

    assert_eq!(
        perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
        Ok(())
    );
    assert_eq!(
        manager.calls.get(3),
        Some(&vec!["start".to_owned(), UNIT.to_owned()])
    );
    assert_eq!(manager.busctl_calls.len(), 8);
    assert_eq!(
        manager.busctl_calls.first(),
        Some(&vec![
            "--system".to_owned(),
            "--timeout=5".to_owned(),
            "get-property".to_owned(),
            "org.freedesktop.systemd1".to_owned(),
            "/org/freedesktop/systemd1/unit/velnor_2dhost_2eservice".to_owned(),
            "org.freedesktop.systemd1.Unit".to_owned(),
            "Conditions".to_owned(),
        ])
    );
    assert_eq!(
        manager.busctl_calls.get(1).and_then(|args| args.last()),
        Some(&"LoadCredential".to_owned())
    );
    assert_eq!(
        manager.busctl_calls.get(2),
        Some(&vec![
            "--system".to_owned(),
            "--timeout=5".to_owned(),
            "get-property".to_owned(),
            "org.freedesktop.systemd1".to_owned(),
            "/org/freedesktop/systemd1/unit/velnor_2dhost_2eservice".to_owned(),
            "org.freedesktop.systemd1.Service".to_owned(),
            ENVIRONMENT_PROPERTY.to_owned(),
        ])
    );
    assert!(manager.busctl_calls.iter().any(|args| {
        args.get(4).is_some_and(|path| path == IDENTITY_OBJECT_PATH)
            && args.last().is_some_and(|property| property == "Conditions")
    }));
}
