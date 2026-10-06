//! Admission rejects omitted, rebound, and unsuccessful action evidence.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{
    ActionReport, ExecuteTaskIds, ExecuteTaskRef, PlanBaseline, PlanGenerator, PlanMatrix,
    PlanObligation, PlanRunner, RunnerSelection, Trust, WorkflowEvent, digest_b3, plan_id_for_run,
};

use super::*;

fn fixture() -> (Plan, MergeRequest) {
    let task = "stack/workload/app/build/docker_build";
    let digest = digest_b3(b"task");
    let mut entry = MatrixEntry::derive(
        "workload",
        task,
        "true",
        &digest,
        serde_json::json!({"configuration":"docker_build", "kind":"build"}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([("build".to_owned(), ExecuteTaskRef::Single(task.to_owned()))]),
        },
        &digest_b3(b"input"),
        "local",
        "workload-app",
    )
    .expect("entry");
    entry.adapter_metadata["action"] = serde_json::json!({
        "id": format!("velnor-action-{}", entry.matrix_key),
        "uses": format!("docker/build-push-action@{}", velnor_actions_actionlint::actions::BUILD_PUSH_ACTION_SHA),
    });
    let plan = Plan {
        producers: Default::default(),
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan ID"),
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
        packages: vec![],
        obligations: vec![PlanObligation {
            task_id: task.to_owned(),
            job_id: "workload-app".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: digest,
            input_digest: digest_b3(b"input"),
            execution_identity: velnor_actions_contract::TaskExecutionIdentity::new(
                &velnor_actions_contract::digest_b3(b"fixture-graph"),
                &velnor_actions_contract::digest_b3(b"fixture-toolchain"),
                &velnor_actions_contract::digest_b3(b"fixture-mbx"),
                &velnor_actions_contract::digest_b3(b"fixture-platform"),
                "default",
            )
            .expect("execution identity"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![task.to_owned()],
        warnings: vec![],
        edges: vec![],
    };
    plan.validate().expect("plan");
    let binding = expected_binding(&plan, &plan.matrix.include[0]).expect("binding");
    let request = serde_json::from_value(serde_json::json!({
        "schema":1, "run_key":"local", "matrix_reports":[], "task_reports":[],
        "required_job_ids":[], "required_jobs":[], "action_begins":[binding],
        "action_reports":[ActionReport { binding: binding.clone(), outcome: ActionOutcome::Success }],
    })).expect("request");
    (plan, request)
}

fn check(plan: &Plan, request: &MergeRequest) -> Signals {
    let mut signals = Signals::default();
    check_actions(plan, request, &mut signals, &mut BTreeSet::new());
    signals
}

#[test]
fn only_complete_bound_success_clears_action_admission() {
    let (plan, request) = fixture();
    let signals = check(&plan, &request);
    assert!(!signals.planning_failed && !signals.failed && !signals.cancelled && !signals.not_run);
    for outcome in [
        ActionOutcome::Failure,
        ActionOutcome::Cancelled,
        ActionOutcome::Skipped,
    ] {
        let (_, mut request) = fixture();
        request.action_reports[0].outcome = outcome;
        let signals = check(&plan, &request);
        assert!(signals.failed || signals.cancelled || signals.not_run);
    }
}

#[test]
fn every_binding_axis_and_inventory_are_required() {
    let (plan, _) = fixture();
    for field in [
        "run_key",
        "source_head",
        "matrix_key",
        "task_id",
        "task_digest",
        "action_id",
        "action_ref",
    ] {
        let (_, mut request) = fixture();
        let mut value = serde_json::to_value(&request.action_reports[0]).expect("value");
        value["binding"][field] = serde_json::json!("wrong");
        request.action_reports[0] = serde_json::from_value(value).expect("report");
        assert!(check(&plan, &request).planning_failed, "{field}");
    }
    let (_, mut request) = fixture();
    request.action_begins.clear();
    assert!(check(&plan, &request).planning_failed);
    let (_, mut request) = fixture();
    request
        .action_reports
        .push(request.action_reports[0].clone());
    assert!(check(&plan, &request).planning_failed);
    let (_, mut request) = fixture();
    request.action_reports.clear();
    assert!(check(&plan, &request).planning_failed);
}

#[test]
fn removing_metadata_does_not_remove_docker_action_requirement() {
    let (mut plan, request) = fixture();
    plan.matrix.include[0].adapter_metadata = serde_json::json!({});
    assert!(check(&plan, &request).planning_failed);
}

fn staged(nested: bool) -> (TempDir, Plan, PathBuf) {
    let (plan, request) = fixture();
    let temp = TempDir::new().expect("temp");
    let entry = &plan.matrix.include[0];
    let artifact = temp.path().join(&entry.artifact_id);
    let home = if nested {
        artifact.join(&entry.artifact_id)
    } else {
        artifact
    }
    .join(&entry.matrix_key);
    fs::create_dir_all(home.join("actions")).expect("dirs");
    fs::write(home.join("matrix-report.json"), "{}").expect("matrix");
    fs::write(
        home.join("actions/begin.json"),
        serde_json::to_vec(&request.action_begins[0]).expect("begin"),
    )
    .expect("file");
    fs::write(
        home.join("actions/report.json"),
        serde_json::to_vec(&request.action_reports[0]).expect("report"),
    )
    .expect("file");
    (temp, plan, home)
}

#[test]
fn direct_and_nested_downloads_preserve_both_action_channels() {
    for nested in [false, true] {
        let (temp, plan, _) = staged(nested);
        let mut errors = Vec::new();
        let (begins, reports) = read_staged_actions(
            &serde_json::to_value(plan).expect("plan"),
            temp.path(),
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(begins.len(), 1);
        assert_eq!(reports.len(), 1);
    }
}

#[test]
fn missing_duplicate_and_oversize_staged_action_files_fail_closed() {
    for contents in [
        None,
        Some(r#"{"binding":{},"binding":{}}"#.to_owned()),
        Some(" ".repeat(16 * 1024 + 1)),
    ] {
        let (temp, plan, home) = staged(false);
        let path = home.join("actions/report.json");
        if let Some(contents) = contents {
            fs::write(path, contents).expect("write")
        } else {
            fs::remove_file(path).expect("remove")
        }
        let mut errors = Vec::new();
        let (_, reports) = read_staged_actions(
            &serde_json::to_value(plan).expect("plan"),
            temp.path(),
            &mut errors,
        );
        assert!(reports.is_empty());
        assert_eq!(errors.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn staged_action_directory_symlink_fails_closed() {
    let (temp, plan, home) = staged(false);
    fs::rename(home.join("actions"), home.join("actual-actions")).expect("rename");
    std::os::unix::fs::symlink(home.join("actual-actions"), home.join("actions")).expect("link");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_actions(
        &serde_json::to_value(plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(begins.is_empty() && reports.is_empty());
    assert!(errors[0].starts_with("symlink_action:"));
}

#[test]
fn covered_entry_has_no_action_execution_inventory() {
    let (mut plan, mut request) = fixture();
    plan.obligations[0].decision = ObligationDecision::CoveredByTrustedBaseline;
    request.action_begins.clear();
    request.action_reports.clear();
    assert!(!check(&plan, &request).planning_failed);
    let (_, full) = fixture();
    assert!(check(&plan, &full).planning_failed);
    let temp = TempDir::new().expect("temp");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_actions(
        &serde_json::to_value(plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(begins.is_empty() && reports.is_empty() && errors.is_empty());
}

#[test]
fn valid_foreign_scope_and_pin_still_fail_exact_binding() {
    let (plan, _) = fixture();
    for (field, value) in [
        ("run_key", "r2-a1".to_owned()),
        (
            "task_id",
            "stack/workload/other/build/docker_build".to_owned(),
        ),
        ("task_digest", digest_b3(b"other task")),
        (
            "action_ref",
            format!("docker/build-push-action@{}", "0".repeat(40)),
        ),
    ] {
        let (_, mut request) = fixture();
        let mut foreign = serde_json::to_value(&request.action_reports[0]).expect("value");
        foreign["binding"][field] = serde_json::json!(value);
        request.action_reports[0] = serde_json::from_value(foreign).expect("foreign report");
        request.action_reports[0]
            .validate()
            .expect("valid foreign scope");
        assert!(check(&plan, &request).planning_failed, "{field}");
    }
    let (_, mut request) = fixture();
    request.action_reports[0].binding.matrix_key = "m-0000000000000000".to_owned();
    request.action_reports[0].binding.action_id = "velnor-action-m-0000000000000000".to_owned();
    request.action_reports[0]
        .validate()
        .expect("valid foreign matrix");
    assert!(check(&plan, &request).planning_failed);
}

#[test]
fn duplicate_begin_and_altered_plan_descriptor_fail_closed() {
    let (plan, mut request) = fixture();
    request.action_begins.push(request.action_begins[0].clone());
    assert!(check(&plan, &request).planning_failed);
    for altered in [
        serde_json::json!({"id":"changed", "uses":"docker/build-push-action@0000000000000000000000000000000000000000"}),
        serde_json::json!({"id":"changed", "uses":"docker/build-push-action@v6"}),
    ] {
        let (mut plan, request) = fixture();
        plan.matrix.include[0].adapter_metadata["action"] = altered;
        assert!(check(&plan, &request).planning_failed);
    }
    let (mut plan, request) = fixture();
    plan.matrix.include[0].adapter_metadata["action"]["extra"] = serde_json::json!(true);
    assert!(check(&plan, &request).planning_failed);
}

#[test]
fn docker_task_cache_reuse_cannot_replace_action_execution() {
    let (mut plan, mut request) = fixture();
    plan.obligations[0].decision = ObligationDecision::ReusedFromTaskCache;
    request.action_begins.clear();
    request.action_reports.clear();
    assert!(check(&plan, &request).planning_failed);
}

#[test]
fn partial_direct_action_evidence_survives_without_matrix_coverage() {
    for terminal_present in [false, true] {
        let (temp, plan, home) = staged(false);
        fs::remove_file(home.join("matrix-report.json")).expect("remove matrix");
        if !terminal_present {
            fs::remove_file(home.join("actions/report.json")).expect("remove terminal");
        }
        let mut errors = Vec::new();
        let (begins, reports) = read_staged_actions(
            &serde_json::to_value(&plan).expect("plan"),
            temp.path(),
            &mut errors,
        );
        assert_eq!(begins.len(), 1);
        assert_eq!(reports.len(), usize::from(terminal_present));
        if terminal_present {
            assert!(errors.is_empty(), "{errors:?}");
        } else {
            assert_eq!(
                errors,
                [format!(
                    "missing_action_report:{}",
                    plan.matrix.include[0].matrix_key
                )]
            );
        }
    }
}

#[test]
fn empty_direct_action_directory_does_not_hide_nested_evidence() {
    let (temp, plan, home) = staged(true);
    fs::remove_file(home.join("matrix-report.json")).expect("remove matrix");
    let entry = &plan.matrix.include[0];
    fs::create_dir_all(
        temp.path()
            .join(&entry.artifact_id)
            .join(&entry.matrix_key)
            .join("actions"),
    )
    .expect("empty direct");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_actions(
        &serde_json::to_value(&plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert_eq!(begins.len(), 1);
    assert_eq!(reports.len(), 1);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn action_evidence_in_both_layouts_is_ambiguous() {
    let (temp, plan, direct) = staged(false);
    let entry = &plan.matrix.include[0];
    let nested = temp
        .path()
        .join(&entry.artifact_id)
        .join(&entry.artifact_id)
        .join(&entry.matrix_key)
        .join("actions");
    fs::create_dir_all(&nested).expect("nested");
    fs::copy(direct.join("actions/begin.json"), nested.join("begin.json"))
        .expect("duplicate begin");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_actions(
        &serde_json::to_value(&plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(begins.is_empty() && reports.is_empty());
    assert_eq!(errors, [format!("ambiguous_action:{}", entry.matrix_key)]);
}
