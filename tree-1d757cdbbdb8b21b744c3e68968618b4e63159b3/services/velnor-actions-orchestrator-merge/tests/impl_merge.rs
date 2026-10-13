use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{MatrixEntry, MatrixReport, Plan};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_merge::merge_checks::partition_task_reports;
use velnor_actions_orchestrator_merge::merge_lenient::lenient_request;
use velnor_actions_orchestrator_merge::required_evidence::{
    diagnostic_without_plan, reported_job_results,
};
use velnor_actions_orchestrator_merge::{MergeRequest, merge_internal_with};
use velnor_actions_orchestrator_merge_ports::{
    BaselineManifest, CoverPort, CoverSinks, Partition, ResourceLimits, ShardProof, Signals,
};

/// Benign cover port: empty partitions, covered entries, passing proofs.
struct FakeCover;

impl CoverPort for FakeCover {
    fn partition_reports<'a>(
        &self,
        _request: &'a MergeRequest,
        _entries: &BTreeMap<&str, &MatrixEntry>,
        _signals: &mut Signals,
        _miss_reasons: &mut BTreeSet<String>,
    ) -> Partition<'a> {
        Partition {
            valid: BTreeMap::new(),
            malformed: 0,
            duplicates: 0,
        }
    }

    fn cover_entry(
        &self,
        _request: &MergeRequest,
        _entry: &MatrixEntry,
        _report: &MatrixReport,
        _obligations: &BTreeMap<&str, &str>,
        _sinks: &mut CoverSinks<'_>,
    ) -> Result<bool, OrchestratorError> {
        Ok(false)
    }

    fn revalidate_coverage(
        &self,
        _plan: &Plan,
        _manifest: Option<&BaselineManifest>,
        _signals: &mut Signals,
        _miss_reasons: &mut BTreeSet<String>,
    ) {
    }

    fn check_entry_shards(
        &self,
        _bases: &BTreeSet<String>,
        _empty_count: u32,
        _proofs: &[ShardProof],
        _obligations: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn validate_budgets(&self, _limits: &ResourceLimits) -> Result<(), String> {
        Ok(())
    }
}

/// Minimal valid request envelope; callers override fields.
fn envelope() -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "matrix_reports": [],
        "required_job_ids": [],
        "required_jobs": [],
    })
}

fn request(value: serde_json::Value) -> MergeRequest {
    serde_json::from_value(value).expect("request parses")
}

#[test]
fn lenient_request_valid_envelope_passes_through() {
    let found = lenient_request(&envelope()).expect("valid envelope");
    assert_eq!(found.run_key, "local");
    assert!(found.assembly_errors.is_empty());
}

#[test]
fn lenient_request_malformed_envelope_is_none() {
    assert!(lenient_request(&serde_json::json!({"schema": 1})).is_none());
    assert!(lenient_request(&serde_json::json!({"run_key": "local"})).is_none());
}

#[test]
fn lenient_request_shape_failure_becomes_assembly_error() {
    let mut raw = envelope();
    raw["plan"] = serde_json::json!(42);
    let found = lenient_request(&raw).expect("envelope survives");
    assert_eq!(found.assembly_errors, vec!["unparsable_plan".to_owned()]);
    assert!(found.plan.is_none());
}

#[test]
fn reported_job_results_adds_missing_markers_sorted() {
    let mut raw = envelope();
    raw["required_job_ids"] = serde_json::json!(["b", "a"]);
    raw["required_jobs"] = serde_json::json!([{"job_id": "b", "conclusion": "success"}]);
    let reported = reported_job_results(&request(raw));
    let ids: Vec<&str> = reported.iter().map(|job| job.job_id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
    assert_eq!(
        format!("{:?}", reported[0].conclusion),
        "Missing",
        "unreported validator never silent"
    );
}

fn task_report(run_key: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "task_report_id": "t",
        "run_key": run_key,
        "event": "push",
        "trust": "trusted",
        "matrix_id": "m",
        "matrix_key": "k",
        "task_id": "t",
        "task_digest": "d",
        "status": "executed",
        "cache": {"layer": "task", "key": "", "result": "not_attempted"},
        "exit_code": 0,
        "outputs": [],
    })
}

#[test]
fn partition_task_reports_wrong_run_mistrusts_scope() {
    let mut raw = envelope();
    raw["task_reports"] = serde_json::Value::Array(vec![task_report("other")]);
    let parsed = request(raw);
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let partition = partition_task_reports(&parsed, &BTreeMap::new(), &mut signals, &mut miss);
    assert_eq!(partition.malformed, 1);
    assert!(signals.not_run);
    assert!(miss.contains("trust_scope_mismatch"));
}

#[test]
fn partition_task_reports_empty_is_clean() {
    let parsed = request(envelope());
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    let partition = partition_task_reports(&parsed, &BTreeMap::new(), &mut signals, &mut miss);
    assert_eq!(partition.malformed, 0);
    assert_eq!(partition.duplicates, 0);
    assert!(partition.valid.is_empty());
    assert!(miss.is_empty());
}

#[test]
fn diagnostic_without_plan_maps_assembly_errors() {
    let mut raw = envelope();
    raw["assembly_errors"] = serde_json::json!(["missing_needs_x", "missing_file_y", "weird"]);
    let report = diagnostic_without_plan(&request(raw), BTreeSet::new()).expect("diagnostic");
    assert!(report.miss_reasons.contains(&"no_entry".to_owned()));
    assert!(report.miss_reasons.contains(&"source_missing".to_owned()));
    assert!(report.miss_reasons.contains(&"cache_corrupt".to_owned()));
}

#[test]
fn merge_internal_with_malformed_request_errors() {
    let err = merge_internal_with(&FakeCover, "{oops").expect_err("malformed errors");
    assert!(err.to_string().contains("malformed_json"), "{err}");
    let err = merge_internal_with(&FakeCover, r#"{"schema":1}"#).expect_err("short errors");
    assert!(err.to_string().contains("malformed_request"), "{err}");
}

#[test]
fn merge_internal_with_wrong_schema_rejected() {
    let mut raw = envelope();
    raw["schema"] = serde_json::json!(2);
    let err = merge_internal_with(&FakeCover, &raw.to_string()).expect_err("schema rejected");
    assert!(err.to_string().contains("unsupported_schema"), "{err}");
}

#[test]
fn merge_internal_with_planless_request_yields_diagnostic() {
    let out = merge_internal_with(&FakeCover, &envelope().to_string()).expect("diagnostic ok");
    let report: serde_json::Value = serde_json::from_str(&out).expect("report json");
    assert_eq!(report["status"], "planning_failed");
    assert_eq!(
        report["miss_reasons"],
        serde_json::json!(["source_missing"])
    );
}
