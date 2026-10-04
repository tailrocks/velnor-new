//! Native helper admission rejects absent authority, cache reuse, and corrupt staging.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation,
    PlanRunner, RunnerSelection, Trust, WorkflowEvent, digest_b3, plan_id_for_run,
};

use super::*;

fn fixture() -> (Plan, MergeRequest) {
    let task = "stack/workload/app/homebrew-tap-local/homebrew_audit";
    let digest = digest_b3(b"task");
    let entry = MatrixEntry::derive(
        "workload",
        task,
        "true",
        &digest,
        serde_json::json!({"configuration":"homebrew_audit", "kind":"homebrew-tap-local"}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "homebrew-tap-local".to_owned(),
                ExecuteTaskRef::Single(task.to_owned()),
            )]),
        },
        &digest_b3(b"input"),
        "local",
        "workload-app",
    )
    .expect("entry");
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
    let request = serde_json::from_value(serde_json::json!({
        "schema":1, "run_key":"local", "matrix_reports":[], "task_reports":[],
        "required_job_ids":[], "required_jobs":[],
    }))
    .expect("request");
    (plan, request)
}

fn check(plan: &Plan, request: &MergeRequest) -> Signals {
    let mut signals = Signals::default();
    check_helpers(plan, request, &mut signals, &mut BTreeSet::new());
    signals
}

#[test]
fn missing_descriptor_and_task_reuse_never_admit_required_native_helper() {
    let (mut plan, request) = fixture();
    assert!(executes_helper(&plan, &plan.matrix.include[0]));
    assert!(check(&plan, &request).planning_failed);
    plan.matrix.include[0].adapter_metadata = serde_json::json!({});
    assert!(check(&plan, &request).planning_failed);
    plan.obligations[0].decision = ObligationDecision::ReusedFromTaskCache;
    assert!(check(&plan, &request).planning_failed);
}

#[test]
fn trusted_baseline_is_only_helper_inventory_exemption() {
    let (mut plan, request) = fixture();
    plan.obligations[0].decision = ObligationDecision::CoveredByTrustedBaseline;
    assert!(!executes_helper(&plan, &plan.matrix.include[0]));
    assert!(!check(&plan, &request).planning_failed);
    let temp = TempDir::new().expect("temp");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_helpers(
        &serde_json::to_value(plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(begins.is_empty() && reports.is_empty() && errors.is_empty());
}

fn staged(nested: bool) -> (TempDir, Plan, PathBuf) {
    let (plan, _) = fixture();
    let temp = TempDir::new().expect("temp");
    let entry = &plan.matrix.include[0];
    let artifact = temp.path().join(&entry.artifact_id);
    let home = if nested {
        artifact.join(&entry.artifact_id)
    } else {
        artifact
    }
    .join(&entry.matrix_key);
    fs::create_dir_all(home.join("helpers")).expect("dirs");
    fs::write(home.join("matrix-report.json"), "{}").expect("matrix");
    fs::write(home.join("helpers/begin.json"), "{}").expect("begin");
    fs::write(home.join("helpers/report.json"), "{}").expect("report");
    (temp, plan, home)
}

#[test]
fn direct_and_nested_downloads_preserve_separate_helper_channels() {
    for nested in [false, true] {
        let (temp, plan, _) = staged(nested);
        let mut errors = Vec::new();
        let (begins, reports) = read_staged_helpers(
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
fn missing_duplicate_key_and_oversize_helper_files_fail_closed() {
    for contents in [
        None,
        Some(r#"{"outcome":"success","outcome":"failure"}"#.to_owned()),
        Some(" ".repeat(1024 * 1024 + 1)),
    ] {
        let (temp, plan, home) = staged(false);
        let path = home.join("helpers/report.json");
        if let Some(contents) = contents {
            fs::write(path, contents).expect("write");
        } else {
            fs::remove_file(path).expect("remove");
        }
        let mut errors = Vec::new();
        let (_, reports) = read_staged_helpers(
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
fn helper_directory_symlink_fails_closed() {
    let (temp, plan, home) = staged(false);
    fs::rename(home.join("helpers"), home.join("actual-helpers")).expect("rename");
    std::os::unix::fs::symlink(home.join("actual-helpers"), home.join("helpers")).expect("link");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_helpers(
        &serde_json::to_value(plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(begins.is_empty() && reports.is_empty());
    assert!(errors[0].starts_with("symlink_helper:"));
}

/// Wire shape may be valid while compiled-owner admission remains denied.
fn prepared_binding(plan: &Plan) -> HelperObligationBinding {
    use velnor_actions_contract::{HelperInvocation, SourceBoundHelper, SourceBoundOperation};
    let operation = SourceBoundOperation::HomebrewPreparation;
    let source =
        SourceBoundHelper::compiled(operation, operation.path(), &"a".repeat(64)).expect("source");
    let invocation =
        HelperInvocation::compiled(source, vec!["root".to_owned()], vec![]).expect("invocation");
    let entry = &plan.matrix.include[0];
    HelperObligationBinding {
        schema: 1,
        run_key: plan.run_key.clone(),
        source_head: plan.head.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        task_digest: plan.obligations[0].task_digest.clone(),
        helper_id: format!("velnor-helper-{}", entry.matrix_key),
        invocation,
        environment: BTreeMap::from([("OWNER".to_owned(), "expected".to_owned())]),
    }
}

#[test]
fn independent_helper_channels_require_exact_binding_and_success() {
    let (plan, mut request) = fixture();
    let binding = prepared_binding(&plan);
    request.helper_begins = vec![binding.clone()];
    request.helper_reports = vec![HelperObligationReport {
        binding: binding.clone(),
        outcome: HelperObligationOutcome::Success,
    }];
    let mut signals = Signals::default();
    check_binding(&binding, &request, &mut signals, &mut BTreeSet::new());
    assert!(!signals.planning_failed && !signals.failed && !signals.cancelled && !signals.not_run);
    for outcome in [
        HelperObligationOutcome::Failure,
        HelperObligationOutcome::Cancelled,
        HelperObligationOutcome::Skipped,
    ] {
        request.helper_reports[0].outcome = outcome;
        let mut signals = Signals::default();
        check_binding(&binding, &request, &mut signals, &mut BTreeSet::new());
        assert!(signals.failed || signals.cancelled || signals.not_run);
    }
    // Valid serialized source references never replace owner qualification.
    assert!(check(&plan, &request).planning_failed);
}

#[test]
fn helper_binding_scope_source_arguments_environment_and_inventory_are_exact() {
    let (plan, mut request) = fixture();
    let binding = prepared_binding(&plan);
    request.helper_begins = vec![binding.clone()];
    let report = HelperObligationReport {
        binding: binding.clone(),
        outcome: HelperObligationOutcome::Success,
    };
    for pointer in [
        "/run_key",
        "/source_head",
        "/task_id",
        "/task_digest",
        "/helper_id",
        "/invocation/helper/source_sha256",
        "/invocation/args/0",
        "/environment/OWNER",
    ] {
        let mut value = serde_json::to_value(&binding).expect("value");
        *value.pointer_mut(pointer).expect("field") = serde_json::json!("wrong");
        request.helper_reports = vec![HelperObligationReport {
            binding: serde_json::from_value(value).expect("binding"),
            outcome: HelperObligationOutcome::Success,
        }];
        let mut signals = Signals::default();
        check_binding(&binding, &request, &mut signals, &mut BTreeSet::new());
        assert!(signals.planning_failed, "{pointer}");
    }
    request.helper_reports = vec![report.clone(), report];
    let mut signals = Signals::default();
    check_binding(&binding, &request, &mut signals, &mut BTreeSet::new());
    assert!(signals.planning_failed);
    request.helper_reports.clear();
    let mut signals = Signals::default();
    check_binding(&binding, &request, &mut signals, &mut BTreeSet::new());
    assert!(signals.planning_failed);
}

#[test]
fn helper_only_partial_layouts_preserve_terminal_failure_without_matrix() {
    for nested in [false, true] {
        let (temp, plan, home) = staged(nested);
        fs::remove_file(home.join("matrix-report.json")).expect("remove matrix");
        fs::remove_file(home.join("helpers/begin.json")).expect("remove begin");
        fs::write(home.join("helpers/report.json"), r#"{"outcome":"failure"}"#).expect("failure");
        let mut errors = Vec::new();
        let (begins, reports) = read_staged_helpers(
            &serde_json::to_value(plan).expect("plan"),
            temp.path(),
            &mut errors,
        );
        assert!(begins.is_empty());
        assert_eq!(reports, vec![serde_json::json!({"outcome":"failure"})]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("missing_helper_begin:"));
    }
}

#[test]
fn empty_direct_helper_directory_does_not_mask_nested_evidence() {
    let (temp, plan, _) = staged(true);
    let entry = &plan.matrix.include[0];
    fs::create_dir_all(
        temp.path()
            .join(&entry.artifact_id)
            .join(&entry.matrix_key)
            .join("helpers"),
    )
    .expect("empty direct");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_helpers(
        &serde_json::to_value(plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(begins.len(), 1);
    assert_eq!(reports.len(), 1);
}

#[test]
fn dual_helper_evidence_layouts_reject_ambiguity() {
    let (temp, plan, _) = staged(false);
    let entry = &plan.matrix.include[0];
    let nested = temp
        .path()
        .join(&entry.artifact_id)
        .join(&entry.artifact_id)
        .join(&entry.matrix_key)
        .join("helpers");
    fs::create_dir_all(&nested).expect("nested");
    fs::write(nested.join("report.json"), r#"{"outcome":"success"}"#).expect("report");
    let mut errors = Vec::new();
    let (begins, reports) = read_staged_helpers(
        &serde_json::to_value(plan).expect("plan"),
        temp.path(),
        &mut errors,
    );
    assert!(begins.is_empty() && reports.is_empty());
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("ambiguous_helper_layout:"));
}

#[cfg(unix)]
#[test]
fn linked_helper_terminal_rejects_even_with_other_valid_evidence() {
    for nested in [false, true] {
        let (temp, plan, home) = staged(nested);
        let report = home.join("helpers/report.json");
        let target = home.join("helpers/report-target.json");
        fs::rename(&report, &target).expect("rename");
        std::os::unix::fs::symlink(&target, &report).expect("link");
        let mut errors = Vec::new();
        let (begins, reports) = read_staged_helpers(
            &serde_json::to_value(plan).expect("plan"),
            temp.path(),
            &mut errors,
        );
        assert!(begins.is_empty() && reports.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("symlink_helper:"));
    }
}
