use super::*;

#[test]
fn start_requires_exact_singleton_systemd_service_environment_before_mutation() {
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

    for environment in [
        b"as 0\n".as_slice(),
        b"as 1 \"PATH=/usr/bin:/bin\"\n".as_slice(),
        b"as 2 \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\" \"EXTRA=value\"\n".as_slice(),
        b"as 1 \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\" \"EXTRA=value\"\n".as_slice(),
        b"s \"PATH=/usr/sbin:/usr/bin:/sbin:/bin\"\n".as_slice(),
    ] {
        let mut manager = FakeManager {
            outputs: [manager_output(true, stopped.clone()), credential_property()].into(),
            environment_outputs: [manager_output(true, environment.to_vec())].into(),
            ..FakeManager::default()
        };

        assert_eq!(
            perform(ServiceAction::Start, &mut manager, DRAIN_TIMEOUT_SECS),
            Err(ServiceFault::ServiceEnvironmentUnavailable)
        );
        assert_eq!(manager.calls.len(), 1);
        assert!(
            !manager
                .calls
                .iter()
                .any(|call| call.first().is_some_and(|verb| verb == "start"))
        );
        assert_eq!(
            manager.busctl_calls.last().and_then(|args| args.last()),
            Some(&ENVIRONMENT_PROPERTY.to_owned())
        );
    }
}
