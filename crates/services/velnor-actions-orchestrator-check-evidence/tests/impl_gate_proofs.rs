//! Proof reads accept only plan-named executed mise entries.

use serde_json::json;
use std::path::Path;
use velnor_actions_orchestrator_check_evidence::gate::read_proofs;

fn entry(stack: &str, task_id: &str) -> serde_json::Value {
    json!({
        "id": "stack:mise|task:g1",
        "matrix_key": "m-0123456789abcdef",
        "stack_id": stack,
        "task_id": task_id,
        "run": "true",
        "task_digest": "b3-0000",
        "adapter_metadata": {},
        "execute_task_ids": {},
        "input_digest": "b3-0000",
        "report_id": "r1",
        "job_id": "job",
        "artifact_id": "velnor-matrix-local-m-0123456789abcdef"
    })
}

fn plan(entries: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"matrix": {"include": entries}})
}

fn tasks(task_id: &str, status: &str) -> Vec<serde_json::Value> {
    vec![json!({"task_id": task_id, "status": status})]
}

#[test]
fn missing_matrix_reads_no_proofs() {
    let mut errors = Vec::new();
    let proofs = read_proofs(&json!({}), Path::new("reports"), &[], &mut errors);
    assert!(proofs.is_empty());
    assert!(errors.is_empty());
}

#[test]
fn unparsable_entries_skip_silently() {
    let mut errors = Vec::new();
    let proofs = read_proofs(
        &plan(vec![json!({"bogus": 1})]),
        Path::new("reports"),
        &[],
        &mut errors,
    );
    assert!(proofs.is_empty());
    assert!(errors.is_empty());
}

#[test]
fn non_mise_and_unexecuted_entries_skip() {
    let mut errors = Vec::new();
    let proofs = read_proofs(
        &plan(vec![
            entry("cargo", "task-1"),
            entry("mise", "task-2"),
            entry("mise", "task-3"),
        ]),
        Path::new("reports"),
        &tasks("task-1", "executed"),
        &mut errors,
    );
    assert!(proofs.is_empty());
    assert!(errors.is_empty());
    let mut errors = Vec::new();
    let proofs = read_proofs(
        &plan(vec![entry("mise", "task-9")]),
        Path::new("reports"),
        &tasks("task-9", "reused"),
        &mut errors,
    );
    assert!(proofs.is_empty());
    assert!(errors.is_empty());
}

#[test]
fn staged_proof_round_trip() {
    let reports = tempfile::TempDir::new().expect("reports");
    let home = reports
        .path()
        .join("velnor-matrix-local-m-0123456789abcdef")
        .join("m-0123456789abcdef");
    std::fs::create_dir_all(home.join("evidence")).expect("dirs");
    std::fs::write(home.join("check-execution.json"), r#"{"schema":1}"#).expect("receipt");
    std::fs::write(home.join("evidence").join("proof.json"), "evidence-text").expect("evidence");
    let mut full = entry("mise", "task-1");
    full["adapter_metadata"] = json!({
        "evidence": {"path": "proof.json", "expected_scenarios": ["s1"]}
    });
    let task_list = tasks("task-1", "executed");
    let mut errors = Vec::new();
    let proofs = read_proofs(&plan(vec![full]), reports.path(), &task_list, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(proofs.len(), 1);
    assert_eq!(proofs[0]["execution"]["schema"], json!(1));
    assert_eq!(proofs[0]["evidence"], json!("evidence-text"));
}
