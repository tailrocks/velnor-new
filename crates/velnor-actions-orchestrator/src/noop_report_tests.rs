//! No-op report-variant tests.
//!
//! Declared via `#[path]` from `noop_report.rs` under `cfg(test)`. Plans
//! are hand-built valid values; bytes must be canonical Rust output.

use std::collections::BTreeMap;
use std::fs;

use crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV;
use tempfile::TempDir;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus, ObligationDecision,
    Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform,
    RunnerSelection, TaskReport, TaskStatus, Trust, WorkflowEvent, matrix_json_bytes,
    plan_id_for_run,
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
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("entry derives");
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        qualification: None,
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

#[test]
fn noop_report_writes_not_selected_with_reason() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged_run(&plan);
    let entry = &plan.matrix.include[0];
    let request = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest: task_digest.clone(),
    };
    let reported = write_noop_report_to("local", CLIPPY, 0, &request, temp.path()).expect("report");
    assert_eq!(reported, 1);
    let expect_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, &task_digest)
            .expect("task report id");
    let (task, matrix) = read_entry(&temp, &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::NotSelected);
    assert_eq!(
        task.not_selected_reason,
        Some(NotSelectedReason::Unsupported)
    );
    assert_eq!(task.exit_code, 0);
    assert_eq!(task.task_digest, task_digest);
    assert_eq!(matrix.status, MatrixStatus::Passed);
    assert_eq!(matrix.not_selected, 1);
    assert_eq!(matrix.report_id, entry.report_id);
}

#[test]
fn noop_request_parsing_is_strict() {
    for (word, reason) in [
        ("upstream_failed", NotSelectedReason::UpstreamFailed),
        ("not_in_plan", NotSelectedReason::NotInPlan),
        ("unsupported", NotSelectedReason::Unsupported),
        ("cancelled_by_policy", NotSelectedReason::CancelledByPolicy),
    ] {
        assert_eq!(parse_not_selected_reason(word).expect("reason"), reason);
    }
    for bad in ["", "success", "Unsupported", "not_selected", "skipped"] {
        assert!(parse_not_selected_reason(bad).is_err(), "{bad:?}");
    }
    assert!(parse_noop_request(None, None).expect("absent").is_none());
    let good_digest = digest(1);
    assert!(
        parse_noop_request(Some("unsupported"), Some(&good_digest))
            .expect("pair")
            .is_some()
    );
    assert!(parse_noop_request(Some("unsupported"), None).is_err());
    assert!(parse_noop_request(None, Some(&good_digest)).is_err());
    assert!(parse_noop_request(Some("bogus"), Some(&good_digest)).is_err());
    assert!(parse_noop_request(Some("unsupported"), Some("b3-short")).is_err());
}

#[test]
fn noop_rejects_contradictions() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged_run(&plan);
    let request = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest: task_digest.clone(),
    };
    let err = write_noop_report_to("local", CLIPPY, 1, &request, temp.path()).expect_err("exit");
    assert!(err.to_string().contains("reason_with_failure"), "{err}");
    let drifted = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest: digest(9),
    };
    let err = write_noop_report_to("local", CLIPPY, 0, &drifted, temp.path()).expect_err("digest");
    assert!(err.to_string().contains("noop_digest_mismatch"), "{err}");
    assert!(
        write_noop_report_to(
            "local",
            "stack/rust/demo/test/default",
            0,
            &request,
            temp.path()
        )
        .is_err()
    );
}

#[test]
fn noop_op_contract_pins_wire_strings() {
    assert_eq!(REPORT_OP, "write-task-report-v1");
    assert_eq!(NOT_SELECTED_REASON_ENV, "VELNOR_NOT_SELECTED_REASON");
    assert_eq!(TASK_DIGEST_ENV, "VELNOR_NOOP_TASK_DIGEST");
    assert_eq!(EXIT_CODE_ENV, "VELNOR_EXIT_CODE");
    assert_eq!(TASK_ID_ENV, "VELNOR_TASK_ID");
}

#[test]
fn noop_rejects_malformed_run_key_before_path_join() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged_run(&plan);
    let request = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest,
    };
    // Symmetry with the exec path: the run key is validated before
    // `load_plan` joins it into a path, so traversal keys never reach
    // the filesystem (unreachable via `resolve_run_key` today, which
    // validates or re-derives digits-only keys).
    for bad in [
        "",
        "LOCAL",
        "r1-a",
        "../evil",
        "local/../../evil",
        "r1-a1/x",
    ] {
        let err = write_noop_report_to(bad, CLIPPY, 0, &request, temp.path()).expect_err("run key");
        assert!(
            err.to_string().contains("malformed_run_key"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn noop_digest_key_is_disjoint_from_exec_digest_key() {
    assert_ne!(
        TASK_DIGEST_ENV, OBLIGATION_TASK_DIGEST_ENV,
        "exec steps bake the obligation digest into every obligation env; aliasing makes the report op fail noop_half_present on every executed task"
    );
}
