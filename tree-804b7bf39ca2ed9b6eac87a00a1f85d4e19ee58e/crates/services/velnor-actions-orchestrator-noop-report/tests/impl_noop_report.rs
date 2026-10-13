//! No-op report surface: parsing, writes, skips, and wire strings.
use std::collections::BTreeMap;

use tempfile::TempDir;
use velnor_actions_contract::plan_id_for_run;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, NotSelectedReason, ObligationDecision, Plan,
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, TaskStatus, Trust,
    WorkflowEvent,
};
use velnor_actions_orchestrator_noop_report::noop_report::{
    NOT_SELECTED_REASON_ENV, NoOpRequest, TASK_DIGEST_ENV, parse_noop_request,
    parse_not_selected_reason, write_noop_report_to, write_skip_reports,
};

const CLIPPY: &str = "stack/rust/demo/clippy/default";
const TEST: &str = "stack/rust/demo/test/default";
const BOUND: u64 = u64::MAX;

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

fn entry_for(task_id: &str, kind: &str, seed: u8, job_id: &str) -> (MatrixEntry, String) {
    let task_digest = digest(seed);
    let entry = MatrixEntry::derive(
        "rust",
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.to_owned()))]),
        },
        &digest(seed + 10),
        "local",
        job_id,
    )
    .expect("entry derives");
    (entry, task_digest)
}

/// Valid two-task fixture plan plus the clippy obligation digest.
fn fixture_plan() -> (Plan, String) {
    let (clippy_entry, clippy_digest) = entry_for(CLIPPY, "clippy", 1, "crate_clippy");
    let (test_entry, test_digest) = entry_for(TEST, "test", 2, "crate_test");
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
        obligations: vec![
            PlanObligation {
                task_id: CLIPPY.to_owned(),
                decision: ObligationDecision::Execute,
                reason: "selected".to_owned(),
                task_digest: clippy_digest.clone(),
                input_digest: digest(11),
                closure_digest: digest(21),
                baseline_proof: None,
            },
            PlanObligation {
                task_id: TEST.to_owned(),
                decision: ObligationDecision::Execute,
                reason: "selected".to_owned(),
                task_digest: test_digest,
                input_digest: digest(12),
                closure_digest: digest(22),
                baseline_proof: None,
            },
        ],
        matrix: PlanMatrix {
            include: vec![clippy_entry, test_entry],
        },
        task_ids: vec![CLIPPY.to_owned(), TEST.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    (plan, clippy_digest)
}

fn staged(plan: &Plan) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let dir = temp.path().join("velnor").join("local");
    std::fs::create_dir_all(&dir).expect("run dir");
    std::fs::write(
        dir.join("plan.json"),
        serde_json::to_string(plan).expect("plan json"),
    )
    .expect("plan file");
    temp
}

fn request(task_digest: String) -> NoOpRequest {
    NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest,
    }
}

#[test]
fn reasons_parse_closed_vocabulary() {
    for (word, reason) in [
        ("upstream_failed", NotSelectedReason::UpstreamFailed),
        ("not_in_plan", NotSelectedReason::NotInPlan),
        ("unsupported", NotSelectedReason::Unsupported),
        ("cancelled_by_policy", NotSelectedReason::CancelledByPolicy),
    ] {
        assert_eq!(parse_not_selected_reason(word).expect("reason"), reason);
    }
}

#[test]
fn unknown_reasons_refuse() {
    for bad in ["", "success", "Unsupported", "skipped"] {
        let err = parse_not_selected_reason(bad).expect_err("unknown refuses");
        assert!(err.to_string().contains("bad_not_selected_reason"), "{err}");
    }
}

#[test]
fn absent_pair_is_no_request() {
    assert!(parse_noop_request(None, None).expect("absent").is_none());
}

#[test]
fn half_present_pairs_refuse() {
    let good_digest = digest(1);
    for (reason, task_digest) in [
        (Some("unsupported"), None),
        (None, Some(good_digest.as_str())),
    ] {
        let err = parse_noop_request(reason, task_digest).expect_err("half refuses");
        assert!(err.to_string().contains("noop_half_present"), "{err}");
    }
}

#[test]
fn malformed_digest_refuses() {
    let err =
        parse_noop_request(Some("unsupported"), Some("b3-short")).expect_err("malformed refuses");
    assert!(!err.to_string().contains("noop_half_present"), "{err}");
}

#[test]
fn noop_writes_not_selected_report() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged(&plan);
    let reported = write_noop_report_to(
        "local",
        CLIPPY,
        0,
        &request(task_digest),
        temp.path(),
        BOUND,
    )
    .expect("report");
    assert_eq!(reported, 1);
    let entry = &plan.matrix.include[0];
    let dir = temp
        .path()
        .join("velnor")
        .join("local")
        .join(&entry.matrix_key);
    assert!(dir.join("matrix-report.json").is_file());
    let tasks: Vec<_> = std::fs::read_dir(dir.join("tasks"))
        .expect("tasks dir")
        .collect();
    assert_eq!(tasks.len(), 1);
}

#[test]
fn digest_mismatch_refuses() {
    let (plan, _) = fixture_plan();
    let temp = staged(&plan);
    let err = write_noop_report_to("local", CLIPPY, 0, &request(digest(9)), temp.path(), BOUND)
        .expect_err("mismatch refuses");
    assert!(err.to_string().contains("noop_digest_mismatch"), "{err}");
}

#[test]
fn reason_with_failure_refuses() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged(&plan);
    let err = write_noop_report_to(
        "local",
        CLIPPY,
        1,
        &request(task_digest),
        temp.path(),
        BOUND,
    )
    .expect_err("contradiction refuses");
    assert!(err.to_string().contains("reason_with_failure"), "{err}");
}

#[test]
fn downstream_self_refuses() {
    let (plan, _) = fixture_plan();
    let temp = staged(&plan);
    let err = write_skip_reports(&plan, CLIPPY, &[CLIPPY.to_owned()], temp.path())
        .expect_err("self refuses");
    assert!(err.to_string().contains("downstream_self"), "{err}");
}

#[test]
fn unknown_downstream_refuses() {
    let (plan, _) = fixture_plan();
    let temp = staged(&plan);
    let err = write_skip_reports(
        &plan,
        CLIPPY,
        &["stack/rust/demo/nope/default".to_owned()],
        temp.path(),
    )
    .expect_err("unknown refuses");
    assert!(err.to_string().contains("task_not_in_plan"), "{err}");
}

#[test]
fn skips_write_not_selected_reports() {
    let (plan, _) = fixture_plan();
    let temp = staged(&plan);
    let reported =
        write_skip_reports(&plan, CLIPPY, &[TEST.to_owned()], temp.path()).expect("skips");
    assert_eq!(reported, 1);
    let entry = &plan.matrix.include[1];
    let task_bytes = std::fs::read(
        temp.path()
            .join("velnor")
            .join("local")
            .join(&entry.matrix_key)
            .join("matrix-report.json"),
    )
    .expect("matrix file");
    let matrix: velnor_actions_contract_workflow::MatrixReport =
        serde_json::from_slice(&task_bytes).expect("matrix json");
    assert_eq!(matrix.not_selected, 1);
    assert_eq!(matrix.tasks[0].status, TaskStatus::NotSelected);
}

#[test]
fn env_keys_pin_wire_strings() {
    assert_eq!(NOT_SELECTED_REASON_ENV, "VELNOR_NOT_SELECTED_REASON");
    assert_eq!(TASK_DIGEST_ENV, "VELNOR_NOOP_TASK_DIGEST");
}
