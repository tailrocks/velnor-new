use super::*;
use mbx_cache_core::{
    AdapterKind, AdapterMeasurement, LinkAttribution, MeasurementEvent, ProcessMeasurement,
    ProcessOutcome, ProcessPurpose, UnitMeasurement,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const SESSION: &str = "11111111-1111-4111-8111-111111111111";

fn identity() -> serde_json::Value {
    json!({"session_id": SESSION, "root_session_id": SESSION,
        "parent_session_id": null, "caller_correlation": "fixture"})
}

fn event(wall: u64) -> MeasurementEvent {
    MeasurementEvent::Process {
        adapter: AdapterKind::Rustc,
        purpose: ProcessPurpose::Work,
        outcome: ProcessOutcome::Succeeded,
        measurement: ProcessMeasurement {
            attempts: 1,
            started: 1,
            observed_wall_ns: wall,
            wall_observations: 1,
        },
        unit: None,
    }
}

fn snapshot(wall: Option<u64>) -> Vec<u8> {
    let mut adapters = BTreeMap::new();
    if let Some(wall) = wall {
        let mut adapter = AdapterMeasurement::default();
        adapter
            .subprocesses
            .entry(ProcessPurpose::Work)
            .or_default()
            .insert(
                ProcessOutcome::Succeeded,
                ProcessMeasurement {
                    attempts: 1,
                    started: 1,
                    observed_wall_ns: wall,
                    wall_observations: 1,
                },
            );
        adapter.units.push(UnitMeasurement {
            identity: None,
            invocations: adapter.invocations.clone(),
            subprocesses: adapter.subprocesses.clone(),
            ..Default::default()
        });
        adapters.insert(AdapterKind::Rustc, adapter);
    }
    let mut coverage = MeasurementCoverage::default();
    coverage.status = MeasurementCoverageStatus::Verified;
    coverage.reason = None;
    serde_json::to_vec(&json!({"schema_version": 1, "completed": true,
    "mbx_version": "fixture-version", "source_base_version": "fixture-base",
    "identity": identity(), "statistics": {"measurement": CompletedMeasurement {
        coverage, measurement_package_generation: None, measurement_package_availability: None,
        measurement_packages: Vec::new(),
        workload_wall_ns: Some(500), cache_post_workload_drain_ns: Some(10),
        adapters, link_wall_ns: None, link_attribution: LinkAttribution::CombinedUnknown,
    }}}))
    .unwrap()
}

fn receipt(id: &str, payload: Option<&MeasurementEvent>) -> Vec<u8> {
    let delivery = payload.map_or_else(
        || json!({"status": "attempted"}),
        |event| {
            json!({"status": "acknowledged", "event": event,
            "event_sha256": hex::encode(Sha256::digest(serde_json::to_vec(event).unwrap()))})
        },
    );
    serde_json::to_vec(
        &json!({"schema_version": 1, "mbx_version": "fixture-version",
        "source_base_version": "fixture-base", "identity": identity(), "event_id": id,
        "adapter": "rustc", "event_kind": "process", "delivery": delivery}),
    )
    .unwrap()
}

#[test]
fn empty_maps_and_no_failed_delivery_never_prove_zero() {
    let result = evaluate(&snapshot(None), &[], &[]);
    assert_eq!(result.coverage.status, MeasurementCoverageStatus::Unknown);
    assert!(
        result
            .reasons
            .contains(&QualificationReason::ManagedDispatchClosureUnavailable)
    );
    assert!(
        result
            .reasons
            .contains(&QualificationReason::CargoArtifactClosureUnavailable)
    );
    assert_eq!(
        result.observed_measurement.unwrap().coverage.status,
        MeasurementCoverageStatus::Unknown
    );
}

#[test]
fn matching_terminal_payloads_verify_fidelity_without_fabricating_authority() {
    let event = event(450);
    let receipts = vec![
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None),
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&event)),
    ];
    let result = evaluate(&snapshot(Some(450)), &receipts, &[]);
    assert_eq!(result.coverage.status, MeasurementCoverageStatus::Unknown);
    assert!(
        !result
            .reasons
            .contains(&QualificationReason::AggregateCountMismatch)
    );
    assert!(
        !result
            .reasons
            .contains(&QualificationReason::EventPayloadFidelityUnavailable)
    );
    let observed = result.observed_measurement.unwrap();
    assert_eq!(observed.coverage.status, MeasurementCoverageStatus::Unknown);
    assert_eq!(
        observed.adapters[&AdapterKind::Rustc].subprocesses[&ProcessPurpose::Work]
            [&ProcessOutcome::Succeeded]
            .observed_wall_ns,
        450
    );
}

#[test]
fn orphan_ack_and_changed_wall_are_independently_rejected() {
    let event = event(450);
    let ack = receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&event));
    let orphan = evaluate(&snapshot(Some(450)), &[ack.clone()], &[]);
    assert!(
        orphan
            .reasons
            .contains(&QualificationReason::AcknowledgmentWithoutEnrollment)
    );
    assert_eq!(
        orphan.coverage.status,
        MeasurementCoverageStatus::Unverified
    );
    let receipts = vec![receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None), ack];
    let mismatched = evaluate(&snapshot(Some(451)), &receipts, &[]);
    assert!(
        mismatched
            .reasons
            .contains(&QualificationReason::AggregateCountMismatch)
    );
    assert_eq!(
        mismatched.coverage.status,
        MeasurementCoverageStatus::Unverified
    );
}

#[test]
fn altered_terminal_digest_and_duplicate_ack_are_rejected() {
    let event = event(450);
    let attempt = receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None);
    let ack = receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&event));
    let mut bad: serde_json::Value = serde_json::from_slice(&ack).unwrap();
    bad["delivery"]["event_sha256"] = json!("0".repeat(64));
    let result = evaluate(
        &snapshot(Some(450)),
        &[attempt.clone(), serde_json::to_vec(&bad).unwrap()],
        &[],
    );
    assert!(
        result
            .reasons
            .contains(&QualificationReason::TerminalDigestMismatch)
    );
    let duplicate = evaluate(&snapshot(Some(450)), &[attempt, ack.clone(), ack], &[]);
    assert!(
        duplicate
            .reasons
            .contains(&QualificationReason::DuplicateDeliveryStage)
    );
}

#[test]
fn overflowing_terminal_walls_cannot_match_a_saturated_owner_total() {
    let first = event(u64::MAX);
    let second = event(1);
    let receipts = vec![
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None),
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&first)),
        receipt("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", None),
        receipt("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", Some(&second)),
    ];
    let result = evaluate(&snapshot(Some(u64::MAX)), &receipts, &[]);
    assert!(
        result
            .reasons
            .contains(&QualificationReason::AggregateOverflow)
    );
    assert_eq!(
        result.coverage.status,
        MeasurementCoverageStatus::Unverified
    );
}

#[test]
fn unavailable_diagnostic_and_attempt_only_preserve_observed_work() {
    let attempt = receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None);
    let diagnostic = serde_json::to_vec(&json!({"schema_version": 1,
        "scope": "mbx_owned_adapters", "reason": "enrollment_write_failed",
        "session_id": SESSION, "root_session_id": SESSION, "caller_correlation": "fixture"}))
    .unwrap();
    let result = evaluate(&snapshot(Some(450)), &[attempt], &[diagnostic]);
    assert!(
        result
            .reasons
            .contains(&QualificationReason::DeliveryIncomplete)
    );
    assert!(
        result
            .reasons
            .contains(&QualificationReason::UnavailableDiagnostic)
    );
    assert!(result.observed_measurement.is_some());
    assert_eq!(
        result.coverage.status,
        MeasurementCoverageStatus::Unverified
    );
}

#[test]
fn forged_unit_counts_and_moved_coordinates_are_rejected() {
    let event = event(450);
    let receipts = vec![
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None),
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&event)),
    ];
    let mut forged: serde_json::Value = serde_json::from_slice(&snapshot(Some(450))).unwrap();
    forged["statistics"]["measurement"]["adapters"]["rustc"]["units"][0]["subprocesses"]["work"]
        ["succeeded"]["observed_wall_ns"] = json!(451);
    let counts = evaluate(&serde_json::to_vec(&forged).unwrap(), &receipts, &[]);
    assert!(
        counts
            .reasons
            .contains(&QualificationReason::UnitAggregateMismatch)
    );
    assert_eq!(
        counts.coverage.status,
        MeasurementCoverageStatus::Unverified
    );
    let mut moved: serde_json::Value = serde_json::from_slice(&snapshot(Some(450))).unwrap();
    moved["statistics"]["measurement"]["adapters"]["rustc"]["units"][0]["identity"] =
        json!(mbx_cache_core::UnitIdentity {
            cargo_unit_id: Some("deadbeef".into()),
            source_path: Some("/moved/lib.rs".into()),
            ..Default::default()
        });
    let coordinates = evaluate(&serde_json::to_vec(&moved).unwrap(), &receipts, &[]);
    assert!(
        coordinates
            .reasons
            .contains(&QualificationReason::UnitAggregateMismatch)
    );
}

#[test]
fn matching_coordinates_do_not_authenticate_forged_package_labels() {
    let identity = mbx_cache_core::UnitIdentity {
        cargo_unit_id: Some("deadbeef".into()),
        manifest_path: Some("/repo/Cargo.toml".into()),
        source_path: Some("/repo/src/lib.rs".into()),
        ..Default::default()
    };
    let mut event = event(450);
    if let MeasurementEvent::Process { unit, .. } = &mut event {
        *unit = Some(identity.clone());
    }
    let receipts = vec![
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None),
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&event)),
    ];
    let mut raw: serde_json::Value = serde_json::from_slice(&snapshot(Some(450))).unwrap();
    let mut forged = identity;
    forged.package_id = Some("unqualified-package-id".into());
    forged.origin = PackageOrigin::Registry;
    forged.source_origin = PackageOrigin::Registry;
    raw["statistics"]["measurement"]["adapters"]["rustc"]["units"][0]["identity"] = json!(forged);
    let result = evaluate(&serde_json::to_vec(&raw).unwrap(), &receipts, &[]);
    assert!(
        !result
            .reasons
            .contains(&QualificationReason::UnitAggregateMismatch)
    );
    assert!(
        result
            .reasons
            .contains(&QualificationReason::UnitProvenanceAuthorityUnavailable)
    );
    assert_eq!(result.coverage.status, MeasurementCoverageStatus::Unknown);
}

#[test]
fn omitted_units_preserve_totals_and_cannot_establish_numeric_fidelity() {
    let event = event(450);
    let receipts = vec![
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None),
        receipt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Some(&event)),
    ];
    let mut raw: serde_json::Value = serde_json::from_slice(&snapshot(Some(450))).unwrap();
    raw["statistics"]["measurement"]["adapters"]["rustc"]["omitted_unit_events"] = json!(1);
    raw["statistics"]["measurement"]["adapters"]["rustc"]["units"] = json!([]);
    let result = evaluate(&serde_json::to_vec(&raw).unwrap(), &receipts, &[]);
    assert!(
        result
            .reasons
            .contains(&QualificationReason::UnitAttributionTruncated)
    );
    assert!(
        result
            .reasons
            .contains(&QualificationReason::UnitNumericFidelityUnavailable)
    );
    assert_eq!(result.coverage.status, MeasurementCoverageStatus::Unknown);
    assert!(result.observed_measurement.is_some());
}

#[test]
fn cleared_socket_diagnostic_is_explicit_delivery_unavailability() {
    let diagnostic = serde_json::to_vec(&json!({"schema_version": 1,
        "scope": "mbx_owned_adapters", "reason": "session_unavailable",
        "session_id": SESSION, "root_session_id": SESSION, "caller_correlation": "fixture"}))
    .unwrap();
    let result = evaluate(&snapshot(None), &[], &[diagnostic]);
    assert!(
        result
            .reasons
            .contains(&QualificationReason::UnavailableDiagnostic)
    );
    assert!(
        !result
            .reasons
            .contains(&QualificationReason::DiagnosticMalformed)
    );
    assert_eq!(
        result.coverage.status,
        MeasurementCoverageStatus::Unverified
    );
}
