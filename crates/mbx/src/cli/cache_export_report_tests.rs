use super::*;

#[test]
fn native_export_report_v2_has_no_legacy_usefulness_alias() {
    let outcome = store::TransferOutcome {
        exported: false,
        actions: 0,
        objects: 0,
        bytes: 0,
    };
    let delta = super::super::cache_comparison::Delta::default();
    let report = export_report(ExportReportInput {
        outcome: &outcome,
        snapshot_budget: Some(100),
        delta: Some(&delta),
        semantic_digest: &"0".repeat(64),
        capture: "captured",
        capture_reason: None,
        usefulness_reason: WorkspaceUsefulnessReason::SchedulerValidityNotProven,
    });
    assert_eq!(report["version"], 2);
    let keys = report
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from([
            "version",
            "budget_refused",
            "snapshot_budget_bytes",
            "exported",
            "actions",
            "objects",
            "bytes",
            "emitted_bundle_useful_delta",
            "workspace_usefulness",
            "delta",
            "semantic_digest",
            "workspace_comparison",
            "workspace_comparison_exclusions",
            "workspace_transport_scope",
            "workspace_capture",
            "workspace_capture_unavailable_reason",
            "workspace_persistence_verified",
            "qualification",
        ])
    );
    assert_eq!(report["emitted_bundle_useful_delta"], false);
    assert_eq!(
        report["workspace_usefulness"],
        serde_json::json!({"status":"unavailable", "reason":"scheduler_validity_not_proven"})
    );
    assert_eq!(report["snapshot_budget_bytes"], 100);
    assert_eq!(report["workspace_persistence_verified"], false);
    for alias in [
        "useful_delta",
        "workspace_useful_delta",
        "workspace_usefulness_unavailable_reason",
    ] {
        assert!(report.get(alias).is_none());
    }
}

#[test]
fn native_export_report_v2_has_only_closed_unavailable_reasons() {
    let outcome = store::TransferOutcome {
        exported: true,
        actions: 1,
        objects: 2,
        bytes: 3,
    };
    for (capture, reason, expected) in [
        (
            "captured",
            WorkspaceUsefulnessReason::SchedulerValidityNotProven,
            "scheduler_validity_not_proven",
        ),
        (
            "unavailable_owner_coverage",
            WorkspaceUsefulnessReason::OwnerCoverageUnavailable,
            "owner_coverage_unavailable",
        ),
        (
            "unavailable_owner_proof",
            WorkspaceUsefulnessReason::OwnerProofUnavailable,
            "owner_proof_unavailable",
        ),
        (
            "unavailable_managed_overlap",
            WorkspaceUsefulnessReason::ManagedOverlap,
            "managed_overlap",
        ),
    ] {
        let report = export_report(ExportReportInput {
            outcome: &outcome,
            snapshot_budget: None,
            delta: None,
            semantic_digest: &"0".repeat(64),
            capture,
            capture_reason: Some("diagnostic text is not a schema reason"),
            usefulness_reason: reason,
        });
        assert_eq!(report["version"], 2);
        assert_eq!(report["emitted_bundle_useful_delta"], true);
        assert_eq!(
            report["workspace_usefulness"],
            serde_json::json!({"status":"unavailable", "reason":expected})
        );
        assert_eq!(
            report["workspace_capture_unavailable_reason"],
            "diagnostic text is not a schema reason"
        );
        assert_eq!(report["workspace_usefulness"].as_object().unwrap().len(), 2);
    }
}
