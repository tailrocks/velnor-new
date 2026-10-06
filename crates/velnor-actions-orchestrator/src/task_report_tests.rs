//! Report-producer tests: parsing, op behavior, merge flip.
//!
//! Declared via `#[path]` from `task_report.rs` under `cfg(test)`. Plans
//! are hand-built valid values (no git fixtures); the flip test stages
//! producer output through request assembly into the merge.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus, ObligationDecision,
    Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, RunnerSelection,
    TaskReport, TaskStatus, Trust, WorkflowEvent, matrix_json_bytes, plan_id_for_run,
};

use super::*;

/// Clippy fixture task ID.
pub(super) const CLIPPY: &str = "stack/rust/demo/clippy/default";
/// Test fixture task ID.
pub(super) const TEST: &str = "stack/rust/demo/test/default";

#[test]
fn exit_codes_accept_the_eight_bit_range() {
    assert_eq!(parse_exit_code("0").expect("zero"), 0);
    assert_eq!(parse_exit_code("1").expect("one"), 1);
    assert_eq!(parse_exit_code("255").expect("max"), 255);
    for bad in ["", " ", "-1", "256", "99999", "0x1", "1\n", "ok"] {
        assert!(parse_exit_code(bad).is_err(), "must reject {bad:?}");
    }
}

#[test]
fn downstream_ids_split_dedupe_and_drop_blanks() {
    assert!(parse_downstream(None).is_empty());
    assert!(parse_downstream(Some("")).is_empty());
    assert_eq!(parse_downstream(Some("b,a,b,, a ,")), ["b", "a"]);
}

#[test]
fn start_stamps_parse_and_measure_or_fail_closed() {
    assert_eq!(parse_start_ms("0"), Some(0));
    assert_eq!(parse_start_ms("1759270000000"), Some(1_759_270_000_000));
    assert_eq!(parse_start_ms("99999999999999999999999999"), None);
    for bad in ["", " ", "-1", "12.5", "0x1", "abc", "1\n", "17909847603N"] {
        assert_eq!(parse_start_ms(bad), None, "must reject {bad:?}");
    }
    assert_eq!(elapsed_ms(None), None, "absent telemetry stays unmeasured");
    assert_eq!(
        elapsed_ms(Some(u64::MAX)),
        None,
        "future stamps stay unmeasured"
    );
    assert!(
        elapsed_ms(Some(1)).is_some_and(|elapsed| elapsed >= 1),
        "past stamps measure"
    );
    let now = now_ms().expect("wall clock");
    assert!(elapsed_ms(Some(now)).is_some(), "present stamps measure");
}

/// One `b3-` digest with every byte set to `byte`.
fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

/// One single-task plan entry plus its obligation digest.
fn entry_for(
    stack: &str,
    task_id: &str,
    kind: &str,
    seed: u8,
    job_id: &str,
) -> (MatrixEntry, String) {
    let task_digest = digest(seed);
    let entry = MatrixEntry::derive(
        stack,
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

/// One execute obligation.
fn obligation_for(task_id: &str, task_digest: String, seed: u8, job_id: &str) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        job_id: job_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest,
        input_digest: digest(seed + 10),
        execution_identity: velnor_actions_contract::TaskExecutionIdentity::new(
            &velnor_actions_contract::digest_b3(b"fixture-graph"),
            &velnor_actions_contract::digest_b3(b"fixture-toolchain"),
            &velnor_actions_contract::digest_b3(b"fixture-mbx"),
            &velnor_actions_contract::digest_b3(b"fixture-platform"),
            "default",
        )
        .expect("execution identity"),
        closure_digest: digest(seed + 20),
        baseline_proof: None,
    }
}

/// Valid two-task fixture plan (clippy plus test).
pub(crate) fn fixture_plan() -> Plan {
    let (clippy_entry, clippy_digest) = entry_for("rust", CLIPPY, "clippy", 1, "rust-clippy");
    let (test_entry, test_digest) = entry_for("rust", TEST, "test", 2, "rust-test");
    let plan = Plan {
        producers: Default::default(),
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        scope: velnor_actions_contract::VerificationScope::Affected,
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
            obligation_for(CLIPPY, clippy_digest, 1, "rust-clippy"),
            obligation_for(TEST, test_digest, 2, "rust-test"),
        ],
        matrix: PlanMatrix {
            include: vec![clippy_entry, test_entry],
        },
        task_ids: vec![CLIPPY.to_owned(), TEST.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    plan
}

/// Staged run dir: `velnor/<run-key>/plan.json` plus `matrix.json`.
pub(super) fn staged_run(plan: &Plan, run_key: &str) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let dir = temp.path().join("velnor").join(run_key);
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
    run_key: &str,
    matrix_key: &str,
    task_report_id: &str,
) -> (TaskReport, MatrixReport) {
    let dir = temp.path().join("velnor").join(run_key).join(matrix_key);
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
fn executed_report_binds_plan_identities() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let entry = &plan.matrix.include[0];
    let obligation = &plan.obligations[0];
    let reported = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        0,
        None,
        &[],
        temp.path(),
    )
    .expect("report");
    assert_eq!(reported, 1);
    let expect_id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &obligation.task_digest,
    )
    .expect("task report id");
    let (task, matrix) = read_entry(&temp, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::Executed);
    assert_eq!(task.exit_code, 0);
    assert_eq!(task.duration_ms, None, "missing stamp stays unmeasured");
    assert_eq!(task.task_digest, obligation.task_digest);
    assert_eq!(task.event, WorkflowEvent::PullRequest);
    assert_eq!(task.duration_ms, None, "unmeasured timing stays absent");
    assert_eq!(task.timing, None, "unmeasured timing stays absent");
    assert_eq!(matrix.status, MatrixStatus::Passed);
    assert_eq!(matrix.report_id, entry.report_id);
    assert_eq!(matrix.executed, 1);
}

#[test]
fn failed_report_marks_entry_failed() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let entry = &plan.matrix.include[0];
    let obligation = &plan.obligations[0];
    let reported = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        3,
        Some(1),
        &[],
        temp.path(),
    )
    .expect("report");
    assert_eq!(reported, 1);
    let expect_id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &obligation.task_digest,
    )
    .expect("task report id");
    let (task, matrix) = read_entry(&temp, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, 3);
    assert!(task.duration_ms.is_some(), "present stamp measures");
    let duration = task.duration_ms.expect("duration");
    let timing = task.timing.expect("measured duration carries timing");
    assert_eq!(timing.task_ms, Some(duration));
    let mut expected = [None; 10];
    expected[2] = Some(duration);
    assert_eq!(timing.slots(), expected, "only task wall is measured");
    assert_eq!(matrix.status, MatrixStatus::Failed);
    assert_eq!(matrix.failed, 1);
}

/// Tofu task ID for the timing-carrying execution case.
const TOFU_VALIDATE: &str = "stack/tofu/dir-737461636b732f61/validate/default";

#[test]
fn tofu_executed_report_carries_measured_timing() {
    let mut plan = fixture_plan();
    let (tofu_entry, digest) = entry_for("tofu", TOFU_VALIDATE, "validate", 7, "tofu-validate");
    plan.matrix.include[1] = tofu_entry;
    let entry = &plan.matrix.include[1];
    plan.obligations[1] = obligation_for(TOFU_VALIDATE, digest.clone(), 7, "tofu-validate");
    plan.task_ids[1] = TOFU_VALIDATE.to_owned();
    plan.validate().expect("mixed plan validates");
    let temp = staged_run(&plan, "local");
    let reported = write_task_report_to(
        "local",
        TOFU_VALIDATE,
        &digest,
        0,
        Some(1),
        &[],
        temp.path(),
    )
    .expect("report");
    assert_eq!(reported, 1);
    let expect_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, &digest)
            .expect("task report id");
    let (task, matrix) = read_entry(&temp, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::Executed);
    let duration = task.duration_ms.expect("duration");
    let timing = task.timing.expect("tofu timing measures");
    assert_eq!(timing.task_ms, Some(duration));
    assert_eq!(matrix.status, MatrixStatus::Passed);
}

#[test]
fn failure_reports_downstream_skips_and_success_reports_none() {
    let plan = fixture_plan();
    let failing = staged_run(&plan, "local");
    let reported = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        1,
        None,
        &[TEST.to_owned()],
        failing.path(),
    )
    .expect("report with skips");
    assert_eq!(reported, 2);
    let entry = &plan.matrix.include[1];
    let obligation = &plan.obligations[1];
    let expect_id = velnor_actions_contract::task_report_id_for_task(
        "local",
        &entry.matrix_key,
        &obligation.task_digest,
    )
    .expect("task report id");
    let (task, matrix) = read_entry(&failing, "local", &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::NotSelected);
    assert_eq!(
        task.not_selected_reason,
        Some(velnor_actions_contract::NotSelectedReason::UpstreamFailed)
    );
    assert_eq!(matrix.not_selected, 1);

    let passing = staged_run(&plan, "local");
    let reported = write_task_report_to(
        "local",
        CLIPPY,
        &fixture_digest(CLIPPY),
        0,
        None,
        &[TEST.to_owned()],
        passing.path(),
    )
    .expect("clean report");
    assert_eq!(reported, 1);
    assert!(
        !passing
            .path()
            .join("velnor")
            .join("local")
            .join(&entry.matrix_key)
            .exists(),
        "success reports no downstream"
    );
}

/// Compiled digest for the original fixture, independent of staged plan changes.
pub(super) fn fixture_digest(task_id: &str) -> String {
    fixture_plan()
        .obligations
        .into_iter()
        .find(|ob| ob.task_id == task_id)
        .map_or_else(
            || velnor_actions_contract::digest_b3(b"unknown-task"),
            |ob| ob.task_digest,
        )
}
