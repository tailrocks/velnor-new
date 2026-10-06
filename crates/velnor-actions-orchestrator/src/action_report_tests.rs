//! Action API outcome and pre-action identity proof.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{CacheResult, ExecuteTaskIds, ExecuteTaskRef, TaskReport};

use super::*;

const TASK: &str = "stack/workload/container/build/docker_build";

/// One executable Docker action with a complete valid plan.
fn fixture() -> (TempDir, Plan, String) {
    let mut plan = crate::task_report::task_report_tests::fixture_plan();
    let mut obligation = plan.obligations.remove(0);
    obligation.task_id = TASK.to_owned();
    obligation.job_id = "workload-container".to_owned();
    let mut entry = MatrixEntry::derive(
        "workload",
        TASK,
        "true",
        &obligation.task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([("build".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()))]),
        },
        &obligation.input_digest,
        "local",
        &obligation.job_id,
    )
    .expect("entry");
    let action = format!("velnor-action-{}", entry.matrix_key);
    entry.adapter_metadata = serde_json::json!({
        "configuration": "docker_build", "kind": "build",
        "action": {"id": action, "uses": format!("docker/build-push-action@{}", velnor_actions_actionlint::actions::BUILD_PUSH_ACTION_SHA)},
    });
    plan.obligations = vec![obligation];
    plan.matrix.include = vec![entry];
    plan.task_ids = vec![TASK.to_owned()];
    plan.validate().expect("plan");
    let temp = TempDir::new().expect("temp");
    fs::create_dir_all(temp.path().join("velnor/local")).expect("run");
    stage(&temp, &plan);
    (temp, plan, action)
}

/// Replace a fixture's staged plan.
fn stage(temp: &TempDir, plan: &Plan) {
    fs::write(
        temp.path().join("velnor/local/plan.json"),
        canonical_json_bytes(plan).expect("plan JSON"),
    )
    .expect("stage");
}

/// Read a terminal coverage report.
fn task_report(temp: &TempDir, plan: &Plan) -> TaskReport {
    let entry = &plan.matrix.include[0];
    let id = velnor_actions_contract::task_report_id_for_task(
        &plan.run_key,
        &entry.matrix_key,
        &entry.task_digest,
    )
    .expect("ID");
    serde_json::from_slice(
        &fs::read(
            temp.path()
                .join("velnor/local")
                .join(&entry.matrix_key)
                .join("tasks")
                .join(format!("{id}.json")),
        )
        .expect("task bytes"),
    )
    .expect("task report")
}

#[test]
fn cold_success_persists_source_binding_and_non_cache_coverage() {
    let (temp, plan, action) = fixture();
    assert_eq!(
        begin_action_report_to("local", TASK, &action, temp.path()).expect("begin"),
        1
    );
    assert_eq!(
        write_action_report_to("local", TASK, &action, ActionOutcome::Success, temp.path())
            .expect("after"),
        1
    );
    let dir = action_dir(temp.path(), &plan, &plan.matrix.include[0]);
    let report: ActionReport =
        serde_json::from_slice(&fs::read(dir.join("report.json")).expect("bytes")).expect("report");
    assert_eq!(report.outcome, ActionOutcome::Success);
    assert_eq!(report.binding.source_head, plan.head);
    assert_eq!(report.binding.run_key, plan.run_key);
    assert_eq!(report.binding.task_digest, plan.obligations[0].task_digest);
    let task = task_report(&temp, &plan);
    assert_eq!(task.status, TaskStatus::Executed);
    assert_eq!(task.exit_code, 0);
    assert_eq!(task.cache.result, CacheResult::NotAttempted);
    assert!(task.duration_ms.is_none());
    assert!(task.outputs.is_empty());
    assert!(
        write_action_report_to("local", TASK, &action, ActionOutcome::Success, temp.path())
            .is_err()
    );
}

#[test]
fn unsuccessful_upstream_outcomes_report_and_fail_closed() {
    for (outcome, status, exit) in [
        (ActionOutcome::Failure, TaskStatus::Failed, 1),
        (ActionOutcome::Cancelled, TaskStatus::Cancelled, 130),
        (ActionOutcome::Skipped, TaskStatus::NotSelected, 1),
    ] {
        let (temp, plan, action) = fixture();
        begin_action_report_to("local", TASK, &action, temp.path()).expect("begin");
        let error = write_action_report_to("local", TASK, &action, outcome, temp.path())
            .expect_err("fails closed");
        assert!(error.to_string().contains("action_not_successful"));
        let task = task_report(&temp, &plan);
        assert_eq!(task.status, status);
        assert_eq!(task.exit_code, exit);
        assert_eq!(task.cache.result, CacheResult::NotAttempted);
        assert_eq!(
            task.not_selected_reason,
            (outcome == ActionOutcome::Skipped).then_some(NotSelectedReason::UpstreamFailed)
        );
        let dir = action_dir(temp.path(), &plan, &plan.matrix.include[0]);
        let report: ActionReport =
            serde_json::from_slice(&fs::read(dir.join("report.json")).expect("bytes"))
                .expect("report");
        assert_eq!(report.outcome, outcome);
    }
}

#[test]
fn missing_begin_cannot_claim_success() {
    let (temp, plan, action) = fixture();
    assert!(
        write_action_report_to("local", TASK, &action, ActionOutcome::Success, temp.path())
            .is_err()
    );
    assert!(
        !action_dir(temp.path(), &plan, &plan.matrix.include[0])
            .join("report.json")
            .exists()
    );
}

#[test]
fn corrupt_or_tampered_begin_cannot_claim_success() {
    for field in [
        "source_head",
        "run_key",
        "task_id",
        "task_digest",
        "action_ref",
        "action_id",
        "schema",
    ] {
        let (temp, plan, action) = fixture();
        begin_action_report_to("local", TASK, &action, temp.path()).expect("begin");
        let path = action_dir(temp.path(), &plan, &plan.matrix.include[0]).join("begin.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("bytes")).expect("JSON");
        value[field] = match field {
            "schema" => serde_json::json!(2),
            "run_key" => serde_json::json!("r1-a1"),
            "task_id" => serde_json::json!("stack/workload/other/build/docker_build"),
            "task_digest" => serde_json::json!(format!("b3-{}", "a".repeat(64))),
            "action_ref" => {
                serde_json::json!(format!("docker/build-push-action@{}", "0".repeat(40)))
            }
            _ => serde_json::json!("tampered"),
        };
        fs::write(&path, serde_json::to_vec(&value).expect("JSON")).expect("tamper");
        assert!(
            write_action_report_to("local", TASK, &action, ActionOutcome::Success, temp.path())
                .is_err(),
            "{field}"
        );
        assert!(!path.with_file_name("report.json").exists());
    }
    let (temp, plan, action) = fixture();
    begin_action_report_to("local", TASK, &action, temp.path()).expect("begin");
    fs::write(
        action_dir(temp.path(), &plan, &plan.matrix.include[0]).join("begin.json"),
        b"{broken",
    )
    .expect("corrupt");
    assert!(
        write_action_report_to("local", TASK, &action, ActionOutcome::Success, temp.path())
            .is_err()
    );
}

#[test]
fn changed_source_or_action_descriptor_invalidates_begin() {
    for source_change in [true, false] {
        let (temp, mut plan, action) = fixture();
        begin_action_report_to("local", TASK, &action, temp.path()).expect("begin");
        if source_change {
            plan.head = "new-source".to_owned();
        } else {
            plan.matrix.include[0].adapter_metadata["action"]["uses"] = serde_json::json!(
                "docker/build-push-action@0000000000000000000000000000000000000000"
            );
        }
        stage(&temp, &plan);
        assert!(
            write_action_report_to("local", TASK, &action, ActionOutcome::Success, temp.path())
                .is_err()
        );
    }
}
