//! Orchestrator core rows: final-report shape (adapter metadata, counts, aggregates).

use velnor_actions_contract_workflow::FinalStatus;

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_core::{merge, merge_request, success_jobs};

#[test]
fn orch_core_adapter_metadata_round_trips() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty(), "fixture must select work");
    for entry in &plan.matrix.include {
        let meta = &entry.adapter_metadata;
        assert!(meta.is_object(), "opaque object");
        for key in ["package_id", "kind", "configuration", "target"] {
            assert!(meta.get(key).is_some(), "{key} in {}", entry.id);
        }
    }
    plan.validate()?;
    Ok(())
}

#[test]
fn orch_core_final_counts_cover_nine_slots() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &serde_json::to_value(&plan)?,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    let counts = serde_json::to_value(&final_report.counts)?;
    for key in [
        "selected",
        "reused",
        "executed",
        "empty_partition",
        "covered",
        "failed",
        "cancelled",
        "blocked",
        "not_run",
    ] {
        assert!(counts.get(key).is_some(), "{key}");
    }
    assert_eq!(final_report.counts.selected as usize, plan.task_ids.len());
    Ok(())
}

#[test]
fn orch_core_final_report_carries_no_cache_inputs() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &serde_json::to_value(&plan)?,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let text = serde_json::to_string(&merge(&request)?)?;
    assert!(!text.contains("cache"), "aggregate only:\n{text}");
    Ok(())
}
