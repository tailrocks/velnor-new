use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::*;

struct StopOrderingManager {
    inner: FakeManager,
    drain_proven: Rc<Cell<bool>>,
    stop_saw_proof: Rc<Cell<Option<bool>>>,
}

impl Manager for StopOrderingManager {
    fn systemctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        if args.first() == Some(&"stop") {
            self.stop_saw_proof.set(Some(self.drain_proven.get()));
        }
        self.inner.systemctl(args)
    }

    fn busctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        self.inner.busctl(args)
    }
}

fn running_unit() -> Vec<u8> {
    unit_snapshot(UnitSnapshot {
        active_state: "active",
        sub_state: "running",
        main_pid: 42,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    })
}

fn stopped_unit() -> Vec<u8> {
    unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout: "1860s",
    })
}

fn successful_stop_manager(drain_proven: Rc<Cell<bool>>) -> StopOrderingManager {
    let mut outputs = Vec::new();
    outputs.extend([
        manager_output(true, running_unit()),
        credential_property(),
        identity_unit_snapshot(),
        empty_jobs(),
        manager_output(true, running_unit()),
        credential_property(),
        identity_unit_snapshot(),
        empty_jobs(),
        manager_output(true, Vec::new()),
        manager_output(true, stopped_unit()),
        credential_property(),
        identity_unit_snapshot(),
        empty_jobs(),
        manager_output(true, stopped_unit()),
    ]);
    StopOrderingManager {
        inner: FakeManager::with_outputs(outputs),
        drain_proven,
        stop_saw_proof: Rc::new(Cell::new(None)),
    }
}

#[test]
fn stop_requires_drain_proof_before_systemd_mutation() {
    let mut manager = successful_stop_manager(Rc::new(Cell::new(false)));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut drain_calls = 0;

    let result = super::super::stop::stop_with(&mut manager, DRAIN_TIMEOUT_SECS, deadline, |_| {
        drain_calls += 1;
        Err(ServiceFault::DrainUnavailable)
    });

    assert_eq!(result, Err(ServiceFault::DrainUnavailable));
    assert_eq!(drain_calls, 1);
    assert_eq!(manager.stop_saw_proof.get(), None);
    assert!(
        !manager
            .inner
            .calls
            .iter()
            .any(|call| call.first().is_some_and(|verb| verb == "stop"))
    );
}

#[test]
fn stop_does_not_switch_users_for_a_unit_outside_the_package_contract() {
    let altered = String::from_utf8_lossy(&running_unit()).replace(
        "ExecStop={ path=/usr/bin/velnor-host",
        "ExecStop={ path=/tmp/other",
    );
    let mut manager = FakeManager::with_outputs([manager_output(true, altered.into_bytes())]);
    let mut drain_called = false;

    let result = super::super::stop::stop_with(
        &mut manager,
        DRAIN_TIMEOUT_SECS,
        Instant::now() + Duration::from_secs(5),
        |_| {
            drain_called = true;
            Ok(())
        },
    );

    assert_eq!(result, Err(ServiceFault::ServiceContract));
    assert!(!drain_called);
    assert_eq!(manager.calls.len(), 1);
    assert_eq!(manager.calls[0][0], "show");
}

#[test]
fn stop_requests_systemd_only_after_proof_and_verifies_final_state() {
    let drain_proven = Rc::new(Cell::new(false));
    let mut manager = successful_stop_manager(Rc::clone(&drain_proven));
    let deadline = Instant::now() + Duration::from_secs(5);

    let result = super::super::stop::stop_with(&mut manager, DRAIN_TIMEOUT_SECS, deadline, |_| {
        drain_proven.set(true);
        Ok(())
    });

    assert_eq!(result, Ok(()));
    assert_eq!(manager.stop_saw_proof.get(), Some(true));
    assert_eq!(
        manager.inner.calls.last(),
        Some(&vec![
            "show".to_owned(),
            "--no-pager".to_owned(),
            format!("--property={}", super::super::SHOW_PROPERTIES),
            UNIT.to_owned()
        ])
    );
    assert_eq!(
        manager
            .inner
            .calls
            .iter()
            .filter(|call| call.first().is_some_and(|verb| verb == "stop"))
            .count(),
        1
    );
}

#[test]
fn service_user_drain_argv_uses_exact_package_paths_and_bounded_wait() {
    let command = super::super::stop::runuser_command(std::path::Path::new(STATE_PATH), 17);
    assert_eq!(command.get_program(), "/usr/sbin/runuser");
    assert_eq!(
        command
            .get_args()
            .map(|argument| argument.to_str().expect("argv is UTF-8"))
            .collect::<Vec<_>>(),
        [
            "--user",
            "velnor",
            "--group",
            "velnor",
            "--supp-group",
            "docker",
            "--",
            "/usr/bin/velnor-host",
            "--config",
            "/etc/velnor-host/host.toml",
            "--state",
            "/var/lib/velnor-host",
            "drain",
            "--wait",
            "--timeout-secs",
            "17",
        ]
    );
}

#[test]
fn expired_stop_deadline_prevents_systemctl_invocation() {
    let deadline = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("monotonic clock has a prior instant");
    let mut manager = super::super::process::Systemctl::bounded_until(deadline);

    let error = manager
        .systemctl(&["stop", UNIT])
        .expect_err("expired deadline");

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}
