use super::*;
use crate::session::completed_report::{CommandRole, WorkloadOutcome};
use mbx_cache_core::{
    AdapterKind, AdapterMeasurement, ProcessMeasurement, ProcessOutcome, ProcessPurpose,
};

#[test]
#[cfg(unix)]
fn observed_work_without_delivery_receipts_is_unverified_and_keeps_wall_estimates() {
    let temporary = tempfile::tempdir().unwrap();
    let identity =
        SessionIdentity::new(CommandRole::CargoBuild, None, None, Some("fixture-1")).unwrap();
    let workload = WorkloadResult {
        outcome: WorkloadOutcome::Failed,
        exit_code: Some(2),
    };
    let mut stats = AgentStats::default();
    stats.session_duration_ns = 900;
    stats.avoided_compiler_duration_ns = 8_000;
    let mut adapter = AdapterMeasurement::default();
    adapter
        .subprocesses
        .entry(ProcessPurpose::Work)
        .or_default()
        .insert(
            ProcessOutcome::Failed,
            ProcessMeasurement {
                attempts: 1,
                started: 1,
                observed_wall_ns: 450,
                wall_observations: 1,
            },
        );
    stats
        .measurement_adapters
        .insert(AdapterKind::Rustc, adapter);
    let path = publish_completed_stats(
        Some(&temporary.path().join("reports")),
        &identity,
        workload,
        Some(500),
        None,
        None,
        None,
        None,
        &stats,
    )
    .unwrap()
    .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(report["workload"]["outcome"], "failed");
    assert_eq!(
        report["statistics"]["measurement"]["coverage"]["scope"],
        "mbx_owned_adapters"
    );
    assert_eq!(
        report["statistics"]["measurement"]["coverage"]["status"],
        "unverified"
    );
    assert_eq!(
        report["statistics"]["measurement"]["coverage"]["reason"],
        "delivery_incomplete"
    );
    assert_eq!(report["statistics"]["measurement"]["workload_wall_ns"], 500);
    assert!(report["statistics"]["measurement"]["cache_post_workload_drain_ns"].is_null());
    assert!(report["statistics"]["measurement"]["link_wall_ns"].is_null());
    assert_eq!(
        report["statistics"]["measurement"]["link_attribution"],
        "combined_unknown"
    );
    assert_eq!(
        report["statistics"]["measurement"]["adapters"]["rustc"]["subprocesses"]["work"]["failed"]
            ["observed_wall_ns"],
        450
    );
    assert_eq!(
        report["statistics"]["cache"]["estimated_compiler_duration_avoided_ns"],
        8_000
    );
    assert_eq!(report["statistics"]["cache"]["session_duration_ns"], 900);
}

#[test]
#[cfg(unix)]
fn absent_adapter_work_observations_remain_unknown_without_a_completeness_claim() {
    let temporary = tempfile::tempdir().unwrap();
    let identity =
        SessionIdentity::new(CommandRole::CargoBuild, None, None, Some("fixture-empty")).unwrap();
    let workload = WorkloadResult {
        outcome: WorkloadOutcome::Succeeded,
        exit_code: Some(0),
    };
    let path = publish_completed_stats(
        Some(&temporary.path().join("reports")),
        &identity,
        workload,
        Some(500),
        None,
        None,
        None,
        None,
        &AgentStats::default(),
    )
    .unwrap()
    .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        report["statistics"]["measurement"]["coverage"]["status"],
        "unknown"
    );
    assert_eq!(
        report["statistics"]["measurement"]["coverage"]["reason"],
        "completeness_not_proven"
    );
    assert!(
        report["statistics"]["measurement"]["adapters"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn absent_directory_does_not_publish_or_fabricate_measurements() {
    let identity = SessionIdentity::new(CommandRole::Other, None, None, None).unwrap();
    let workload = WorkloadResult {
        outcome: WorkloadOutcome::Unknown,
        exit_code: None,
    };
    assert!(
        publish_completed_stats(
            None,
            &identity,
            workload,
            None,
            None,
            None,
            None,
            None,
            &AgentStats::default()
        )
        .unwrap()
        .is_none()
    );
}
