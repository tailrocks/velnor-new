//! No-op report-variant tests.
//!
//! Declared via `#[path]` from `noop_report.rs` under `cfg(test)`. Plans
//! are hand-built valid values; bytes must be canonical Rust output.

use std::collections::BTreeMap;
use std::fs;

use crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV;
use tempfile::TempDir;
use velnor_actions_contract::plan_id_for_run;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus, ObligationDecision,
    Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, TaskReport,
    TaskStatus, Trust, WorkflowEvent, matrix_json_bytes,
};

use super::*;
use crate::task_report::{EXIT_CODE_ENV, REPORT_OP, TASK_ID_ENV};

/// Clippy fixture task ID.
const CLIPPY: &str = "stack/rust/demo/clippy/default";

/// One `b3-` digest with every byte set to `byte`.
fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

/// Valid single-task fixture plan plus its obligation digest.
fn fixture_plan() -> (Plan, String) {
    let task_digest = digest(1);
    let entry = MatrixEntry::derive(
        "rust",
        CLIPPY,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "clippy".to_owned(),
                ExecuteTaskRef::Single(CLIPPY.to_owned()),
            )]),
        },
        &digest(11),
        "local",
        "rust-demo",
    )
    .expect("entry derives");
    let plan = Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: CLIPPY.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: task_digest.clone(),
            input_digest: digest(11),
            closure_digest: digest(21),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![CLIPPY.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    (plan, task_digest)
}

/// Staged run dir holding `plan.json` plus `matrix.json`.
fn staged_run(plan: &Plan) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let dir = temp.path().join("velnor").join("local");
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(
        dir.join("plan.json"),
        serde_json::to_string(plan).expect("plan json"),
    )
    .expect("plan file");
    fs::write(
        dir.join("matrix.json"),
        matrix_json_bytes(&plan.matrix).expect("matrix json"),
    )
    .expect("matrix file");
    temp
}

/// Read and validate one staged task report plus its aggregate.
fn read_entry(
    temp: &TempDir,
    matrix_key: &str,
    task_report_id: &str,
) -> (TaskReport, MatrixReport) {
    let dir = temp.path().join("velnor").join("local").join(matrix_key);
    let matrix_bytes = fs::read(dir.join("matrix-report.json")).expect("matrix file");
    let task_bytes =
        fs::read(dir.join("tasks").join(format!("{task_report_id}.json"))).expect("task file");
    let matrix: MatrixReport = serde_json::from_slice(&matrix_bytes).expect("matrix json");
    let task: TaskReport = serde_json::from_slice(&task_bytes).expect("task json");
    matrix.validate().expect("matrix validates");
    task.validate().expect("task validates");
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&matrix).expect("canonical matrix"),
        matrix_bytes,
        "matrix bytes are canonical Rust output"
    );
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&task).expect("canonical task"),
        task_bytes,
        "task bytes are canonical Rust output"
    );
    (task, matrix)
}

mod noop_report_tests;
