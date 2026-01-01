use super::super::contract::{
    SYSTEMD_STOP_TIMEOUT_SECS, SYSTEMD_STOP_TIMEOUT_USEC, verify_package_contract,
};
use super::super::systemd::{StopTimeout, parse_snapshot, parse_stop_timeout};
use super::*;
use velnor_runner_host::MAX_LINUX_DRAIN_TIMEOUT_SECS;

fn parsed_unit(timeout: &str) -> super::super::systemd::UnitSnapshot {
    let bytes = unit_snapshot(UnitSnapshot {
        active_state: "inactive",
        sub_state: "dead",
        main_pid: 0,
        control_pid: 0,
        result: "success",
        stop_code: "(null)",
        stop_status: "0/0",
        timeout,
    });
    let mut snapshot = parse_snapshot(&bytes).expect("complete unit snapshot");
    snapshot.identity_marker_condition_matches = true;
    snapshot
}

#[test]
fn package_contract_pins_the_finite_stop_window_and_config_cap() {
    assert_eq!(SYSTEMD_STOP_TIMEOUT_SECS, 1860);
    assert_eq!(SYSTEMD_STOP_TIMEOUT_USEC, 1_860_000_000);
    assert_eq!(MAX_LINUX_DRAIN_TIMEOUT_SECS, 1800);
    assert_eq!(
        parse_stop_timeout("1860s"),
        Some(StopTimeout::Finite(SYSTEMD_STOP_TIMEOUT_USEC))
    );
    assert_eq!(
        verify_package_contract(&parsed_unit("1860s"), MAX_LINUX_DRAIN_TIMEOUT_SECS,),
        Ok(())
    );
    assert_eq!(
        verify_package_contract(&parsed_unit("1860s"), MAX_LINUX_DRAIN_TIMEOUT_SECS + 1),
        Err(ServiceFault::InvalidConfig)
    );
}

#[test]
fn package_contract_rejects_timeout_drift_and_infinity() {
    for timeout in ["1859s", "1861s", "infinity"] {
        assert_eq!(
            verify_package_contract(&parsed_unit(timeout), 900),
            Err(ServiceFault::ServiceContract),
            "unexpected acceptance for effective TimeoutStopSec={timeout}"
        );
    }
}
