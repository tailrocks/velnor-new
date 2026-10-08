use super::*;

#[test]
fn start_rejects_a_missing_or_redirected_systemd_credential_before_start() {
    let valid = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    });
    for credential in [
        manager_output(true, b"a(ss) 0\n".to_vec()),
        manager_output(true, b"a(ss) 1 \"github-token\" \"/tmp/token\"\n".to_vec()),
        manager_output(true, Vec::new()),
        manager_output(
            true,
            b"a(ss) 2 \"github-token\" \"/etc/velnor-host/github-token\" \"other\" \"/tmp/other\"\n".to_vec(),
        ),
        manager_output(false, Vec::new()),
    ] {
        let mut manager = FakeManager::with_outputs([
            manager_output(true, valid.clone()),
            credential,
        ]);
        assert_eq!(
            perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
            Err(ServiceFault::CredentialUnavailable)
        );
        assert_eq!(manager.calls.len(), 1);
        assert_eq!(manager.busctl_calls.len(), 2);
    }
}
