use super::*;
use crate::launch::resource_probe::inspect;
use crate::launch::resource_probe::sample::Observation;
use crate::worker::ResourceBudgetConfig;
use bollard::models::ContainerInspectResponse;
use serde_json::json;
use std::time::Instant;

#[test]
fn ancestor_cpu_and_inspected_pid_gate_start_permit() -> Result<(), String> {
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 1_000,
        runner_memory_bytes: 2_147_483_648,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 6_442_450_944,
        operational_memory_reserve_bytes: 1_073_741_824,
    }
    .validate()
    .map_err(|error| error.to_string())?;
    let required_cpu = budget
        .pair()
        .cpu_millicores
        .checked_add(budget.admission_reserve().cpu_millicores)
        .ok_or_else(|| "The CPU requirement is too large.".to_owned())?;
    assert_eq!(required_cpu, 3_000);
    let mut constrained = fixture_record();
    constrained.memory_available_bytes = 64 * GIB;
    constrained.cpu_ancestor_limits[1].cpu_quota_micros = Some(250_000);
    let constrained = observation(constrained, 18, 1234)?;
    assert!(constrained.into_start_permit(budget).is_none());

    let mut mismatched = fixture_record();
    mismatched.memory_available_bytes = 64 * GIB;
    assert!(
        Observation::new(
            mismatched,
            18,
            4321,
            64 * GIB,
            "selected-engine".to_owned(),
            "a".repeat(64),
            "b".repeat(32),
            Instant::now(),
        )
        .is_none()
    );

    let mut namespace_mismatch = fixture_record();
    namespace_mismatch.probe_namespace_pid = 4321;
    assert!(
        Observation::new(
            namespace_mismatch,
            18,
            1234,
            64 * GIB,
            "selected-engine".to_owned(),
            "a".repeat(64),
            "b".repeat(32),
            Instant::now(),
        )
        .is_none()
    );

    let mut correct_record = fixture_record();
    correct_record.memory_available_bytes = 64 * GIB;
    let permit = observation(correct_record, 18, 1234)?
        .into_start_permit(budget)
        .ok_or_else(|| "The fresh capacity sample must give an admission permit.".to_owned())?;
    assert!(super::super::consume_permit(
        permit,
        "selected-engine",
        &"a".repeat(64)
    ));
    Ok(())
}

#[test]
fn inspected_pid_requires_a_running_container_and_positive_state_pid() -> Result<(), String> {
    let running = inspect_state("running", true, Some(8123))?;
    assert_eq!(inspect::running_host_pid(&running), Some(8123));

    for (status, is_running, pid) in [
        ("created", false, Some(8123)),
        ("exited", false, Some(8123)),
        ("running", false, Some(8123)),
        ("running", true, Some(0)),
        ("running", true, None),
    ] {
        assert_eq!(
            inspect::running_host_pid(&inspect_state(status, is_running, pid)?),
            None
        );
    }
    Ok(())
}

fn observation(record: ProbeRecord, ncpu: u32, inspected_pid: u32) -> Result<Observation, String> {
    Observation::new(
        record,
        ncpu,
        inspected_pid,
        64 * GIB,
        "selected-engine".to_owned(),
        "a".repeat(64),
        "b".repeat(32),
        Instant::now(),
    )
    .ok_or_else(|| "The correct record and matching PID must give an observation.".to_owned())
}

fn inspect_state(
    status: &str,
    running: bool,
    pid: Option<i64>,
) -> Result<ContainerInspectResponse, String> {
    let mut state = serde_json::Map::from_iter([
        ("Status".to_owned(), json!(status)),
        ("Running".to_owned(), json!(running)),
    ]);
    if let Some(pid) = pid {
        state.insert("Pid".to_owned(), json!(pid));
    }
    serde_json::from_value(json!({"State": state})).map_err(|error| error.to_string())
}
