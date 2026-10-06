//! F2 decision tests: classification, ownership, paths, provenance.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    FinalStatus, NotSelectedReason, ObligationDecision, TaskStatus, Trust, WorkflowEvent,
    digest_b3, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_orchestrator::decisions::{
    CacheHit, MetadataFailure, NotSelectedInputs, ObligationInputs, baseline_expired,
    classify_obligation, dedupe_sorted, not_selected_report, omission_ledger, plan_artifact_dir,
    plan_json_path, selection_broadens_for_path,
};

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_plansel::merge_status;

/// Baseline inputs: eligible for reuse or coverage.
fn open_inputs() -> ObligationInputs {
    ObligationInputs {
        schema_known: true,
        undeclared_inputs: false,
        nondeterministic: false,
        inputs_controlled: false,
        cache_hit: CacheHit::Execute,
        baseline_covered: false,
        restore_miss_reason: None,
    }
}

#[test]
fn offline_stderr_aborts_preparation() {
    use velnor_actions_orchestrator::decisions::classify_metadata_failure;
    for stderr in [
        "error: failed to download `serde v1.0.0`\nnetwork unreachable",
        "warning: spurious network error, retrying",
        "error: you are using --offline but `serde` is not downloaded",
        "error: could not resolve `https://github.com/x/y`",
        "error: connection timed out after 30s",
    ] {
        assert_eq!(
            classify_metadata_failure(stderr),
            MetadataFailure::Incomplete,
            "{stderr}"
        );
    }
    for stderr in [
        "error: failed to parse manifest at `Cargo.toml`\nexpected `[package]`",
        "error: no targets specified in the manifest",
    ] {
        assert_eq!(
            classify_metadata_failure(stderr),
            MetadataFailure::Malformed,
            "{stderr}"
        );
    }
}

#[test]
fn every_obligation_classifies_with_reason() {
    let decide = |inputs: &ObligationInputs| classify_obligation(inputs);
    let mut open = open_inputs();
    assert_eq!(decide(&open).0, ObligationDecision::Execute);
    open.schema_known = false;
    assert_eq!(
        decide(&open),
        (ObligationDecision::Execute, "unknown_extension_schema")
    );
    open = open_inputs();
    open.undeclared_inputs = true;
    assert_eq!(
        decide(&open),
        (ObligationDecision::Execute, "undeclared_inputs")
    );
    open.undeclared_inputs = false;
    open.nondeterministic = true;
    assert_eq!(
        decide(&open),
        (ObligationDecision::Execute, "always_run_dynamic_inputs")
    );
    open.inputs_controlled = true;
    open.cache_hit = CacheHit::Verified;
    assert_eq!(
        decide(&open),
        (
            ObligationDecision::ReusedFromTaskCache,
            "reused_from_task_cache"
        )
    );
    open.cache_hit = CacheHit::Execute;
    open.baseline_covered = true;
    assert_eq!(
        decide(&open),
        (
            ObligationDecision::CoveredByTrustedBaseline,
            "covered_by_trusted_baseline"
        )
    );
    open.baseline_covered = false;
    open.restore_miss_reason = Some("cache_corrupt");
    assert_eq!(
        decide(&open),
        (ObligationDecision::Execute, "cache_corrupt")
    );
}

#[test]
fn not_selected_reason_validates_iff_present() -> TestResult {
    let task_id = "stack/rust/root/clippy/default";
    let matrix_id = matrix_id_for_task_group("rust", task_id)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    let digest = digest_b3(b"task");
    let inputs = NotSelectedInputs {
        run_key: "local",
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: &matrix_id,
        matrix_key: &matrix_key,
        task_id,
        task_digest: &digest,
        reason: NotSelectedReason::UpstreamFailed,
    };
    let report = not_selected_report(&inputs)?;
    assert_eq!(report.status, TaskStatus::NotSelected);
    assert_eq!(
        report.not_selected_reason,
        Some(NotSelectedReason::UpstreamFailed)
    );
    let mut missing = report.clone();
    missing.not_selected_reason = None;
    assert!(missing.validate().is_err(), "reason required");
    let mut spurious = report.clone();
    spurious.status = TaskStatus::Executed;
    assert!(spurious.validate().is_err(), "reason forbidden");
    Ok(())
}

#[test]
fn broaden_paths_classify_global_and_outside() {
    for path in [
        ".velnor/config.toml",
        ".velnor/",
        ".github/workflows/ci.yml",
    ] {
        assert_eq!(
            selection_broadens_for_path(path),
            Some("global_config_changed:selecting_all"),
            "{path}"
        );
    }
    for path in ["/etc/passwd", "../escape/Cargo.toml", "a/../../b"] {
        assert_eq!(
            selection_broadens_for_path(path),
            Some("outside_project_path:selecting_all"),
            "{path}"
        );
    }
    for path in [
        "alpha/src/lib.rs",
        "Cargo.toml",
        "beta/Cargo.toml",
        "mise.toml",
    ] {
        assert_eq!(selection_broadens_for_path(path), None, "{path}");
    }
}

#[test]
fn omission_ledger_explains_every_skip() {
    let all = [
        "b-task".to_owned(),
        "a-task".to_owned(),
        "c-task".to_owned(),
    ];
    let selected = BTreeSet::from(["b-task".to_owned()]);
    let ledger = omission_ledger(&all, &selected);
    assert_eq!(ledger.len(), 2);
    assert!(
        ledger
            .iter()
            .all(|omission| omission.reason == "not_affected")
    );
    assert_eq!(ledger[0].task_id, "a-task");
    assert_eq!(ledger[1].task_id, "c-task");
    let everything = BTreeSet::from(all.clone());
    assert_eq!(
        omission_ledger(&all, &everything),
        [] as [velnor_actions_orchestrator::decisions::TaskOmission; 0]
    );
}

#[test]
fn plan_artifact_paths_reject_traversal() -> TestResult {
    let root = std::path::Path::new("$RUNNER_TEMP/velnor");
    assert_eq!(
        plan_json_path(root, "local")?,
        root.join("local").join("plan.json")
    );
    assert_eq!(plan_artifact_dir(root, "local")?, root.join("local"));
    for bad in ["", "../escape", "a/b", "has space", "dot."] {
        assert!(plan_json_path(root, bad).is_err(), "{bad}");
        assert!(plan_artifact_dir(root, bad).is_err(), "{bad}");
    }
    Ok(())
}

#[test]
fn baselines_expire_on_schedule() {
    assert!(!baseline_expired(None, u64::MAX));
    assert!(!baseline_expired(Some(200), 199));
    assert!(baseline_expired(Some(200), 200));
    assert!(baseline_expired(Some(200), 201));
}

#[test]
fn exact_base_run_filter_pins_provenance() {
    use velnor_actions_orchestrator::run_select::{
        SelectedBaseRun, select_baseline_artifact, select_exact_base_run,
    };
    let base = "a".repeat(40);
    let other = "b".repeat(40);
    let runs = serde_json::json!([
        {"databaseId": 1, "headSha": other, "headBranch": "t", "event": "push", "conclusion": "success", "attempt": 1},
        {"databaseId": 2, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "failure", "attempt": 1},
        {"databaseId": 3, "headSha": base, "headBranch": "t", "event": "pull_request", "conclusion": "success", "attempt": 1},
        {"databaseId": 4, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "success"},
        {"databaseId": 5, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "success", "attempt": 3},
    ]);
    assert_eq!(
        select_exact_base_run(&runs.to_string(), &base, "t"),
        Ok(SelectedBaseRun {
            run_id: 5,
            attempt: 3
        })
    );
    assert!(select_exact_base_run(&runs.to_string(), &"c".repeat(40), "t").is_err());
    assert!(select_exact_base_run("not json", &base, "t").is_err());
    assert!(select_exact_base_run("[]", &base, "t").is_err());
    let unattested = serde_json::json!([
        {"databaseId": 6, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "success"},
        {"databaseId": 7, "headSha": base, "headBranch": "t", "event": "push", "conclusion": "success", "attempt": 0},
    ]);
    assert!(
        select_exact_base_run(&unattested.to_string(), &base, "t").is_err(),
        "runs without attempt evidence never select"
    );
    let listed = serde_json::json!({"artifacts": [
        {"id": 8, "name": "other", "expired": false},
        {"id": 9, "name": "velnor-baseline-x", "expired": true},
        {"id": 10, "name": "velnor-baseline-x", "expired": false},
    ]});
    assert_eq!(
        select_baseline_artifact(&listed.to_string(), "velnor-baseline-x"),
        Ok(10)
    );
    assert!(select_baseline_artifact(&listed.to_string(), "missing").is_err());
    assert!(select_baseline_artifact("not json", "velnor-baseline-x").is_err());
    let array = serde_json::json!([{"databaseId": 11, "name": "n", "expired": false}]);
    assert_eq!(select_baseline_artifact(&array.to_string(), "n"), Ok(11));
}

#[test]
fn dedupe_stage_reports_conflicts() {
    let (unique, dupes) = dedupe_sorted(&["b".to_owned(), "a".to_owned(), "b".to_owned()]);
    assert_eq!(unique, ["a", "b"]);
    assert_eq!(dupes, ["b"]);
    let (unique, dupes) = dedupe_sorted(&["a".to_owned(), "b".to_owned()]);
    assert_eq!(unique, ["a", "b"]);
    assert_eq!(dupes, [] as [std::string::String; 0]);
}

#[test]
fn sequential_reference_merges_clean() -> TestResult {
    use velnor_actions_orchestrator::schedule::sequential_reference;
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let extra = serde_json::json!({"reference_task_ids": sequential_reference(&plan.task_ids)});
    assert_eq!(
        merge_status(&plan, &reports, &[], &extra)?,
        FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn not_selected_tasks_fold_to_blocked() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    let first = reports.first_mut().ok_or("report")?;
    first.tasks[0].status = velnor_actions_contract::TaskStatus::NotSelected;
    first.executed = 0;
    first.not_selected = 1;
    first.validate()?;
    assert_eq!(
        merge_status(&plan, &reports, &[], &serde_json::json!({}))?,
        FinalStatus::Blocked
    );
    Ok(())
}
