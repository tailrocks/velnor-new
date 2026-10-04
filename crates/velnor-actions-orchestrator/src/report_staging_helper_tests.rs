//! Exact native helper evidence stays in the report-only payload.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use velnor_actions_contract::{ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, Plan};

use super::*;

fn helper_plan() -> Plan {
    let mut plan = crate::task_report::task_report_tests::fixture_plan();
    let task = "stack/workload/tap/homebrew-tap-local/homebrew_audit";
    let mut obligation = plan.obligations.remove(0);
    obligation.task_id = task.to_owned();
    obligation.job_id = "workload-tap".to_owned();
    let entry = MatrixEntry::derive(
        "workload",
        task,
        "true",
        &obligation.task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "homebrew-tap-local".to_owned(),
                ExecuteTaskRef::Single(task.to_owned()),
            )]),
        },
        &obligation.input_digest,
        &plan.run_key,
        &obligation.job_id,
    )
    .expect("entry without helper descriptor");
    plan.obligations = vec![obligation];
    plan.matrix.include = vec![entry];
    plan.task_ids = vec![task.to_owned()];
    plan.validate().expect("helper plan");
    plan
}

#[test]
fn required_helper_partial_bytes_survive_descriptor_and_matrix_absence() {
    let plan = helper_plan();
    let temp = tempfile::tempdir().expect("temp");
    let run = temp.path().join("velnor/local");
    let home = PathBuf::from(&plan.matrix.include[0].matrix_key);
    fs::create_dir_all(run.join(&home).join("helpers")).expect("helper dir");
    fs::write(
        run.join("plan.json"),
        serde_json::to_vec(&plan).expect("plan bytes"),
    )
    .expect("plan");
    let begin = home.join("helpers/begin.json");
    let report = home.join("helpers/report.json");
    let failed = b"{\"outcome\":\"failure\",\"partial\":true}";
    fs::write(run.join(&begin), b"begin evidence").expect("begin");
    fs::write(run.join(&report), failed).expect("failed report");
    fs::write(run.join(&home).join("helpers/owned-env.json"), b"private").expect("noise");
    assert_eq!(stage_reports_to("local", temp.path()).expect("stage"), 2);
    let payload = temp.path().join("velnor/report-payload/local");
    assert_eq!(
        fs::read(payload.join(&begin)).expect("begin bytes"),
        b"begin evidence"
    );
    assert_eq!(
        fs::read(payload.join(&report)).expect("report bytes"),
        failed
    );
    assert!(!payload.join(&home).join("matrix-report.json").exists());
    assert!(!payload.join(&home).join("helpers/owned-env.json").exists());
}

#[test]
fn missing_helper_terminal_stays_missing() {
    let plan = helper_plan();
    let temp = tempfile::tempdir().expect("temp");
    let run = temp.path().join("velnor/local");
    let home = PathBuf::from(&plan.matrix.include[0].matrix_key);
    fs::create_dir_all(run.join(&home).join("helpers")).expect("helper dir");
    fs::write(
        run.join("plan.json"),
        serde_json::to_vec(&plan).expect("plan bytes"),
    )
    .expect("plan");
    fs::write(run.join(&home).join("helpers/begin.json"), b"partial").expect("begin");
    assert_eq!(stage_reports_to("local", temp.path()).expect("stage"), 1);
    let payload = temp.path().join("velnor/report-payload/local").join(&home);
    assert!(payload.join("helpers/begin.json").exists());
    assert!(!payload.join("helpers/report.json").exists());
}
