//! P01/P02 regression tests: closed required evidence, obligation universe.

use crate::impl_common::git_fixture;

use serde_json::json;
use velnor_actions_contract::{FinalStatus, JobConclusion, ObligationDecision};
use velnor_actions_orchestrator::{assemble_merge_request, merge_internal};

use crate::impl_common::{TestResult, make_repo, passing_reports, plan_for_source_change};
use crate::impl_orch_core::{merge, merge_request, success_jobs};
use crate::impl_orch_plansel::{
    BUMP, anchor_repo, commit, entries_for, make_ws, manifest_for, plan_at, put,
};

/// Merge status for one hand-built request value.
fn status_of(request: &serde_json::Value) -> Result<FinalStatus, Box<dyn std::error::Error>> {
    Ok(
        serde_json::from_str::<velnor_actions_contract::FinalReport>(&merge_internal(
            &request.to_string(),
        )?)?
        .status,
    )
}

#[test]
fn empty_required_inventory_fails_closed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &plan_value,
        &matrix,
        &serde_json::to_value(&reports)?,
        &serde_json::json!([]),
    );
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"no_entry".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

#[test]
fn validator_results_must_match_inventory_exactly() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let inventory = json!(["plan", "actionlint"]);
    let plan_value = serde_json::to_value(&plan)?;
    let run = |jobs: serde_json::Value| {
        let mut request = merge_request(
            &plan_value,
            &matrix,
            &serde_json::to_value(&reports)?,
            &jobs,
        );
        request["required_job_ids"] = inventory.clone();
        merge(&request)
    };

    // A missing validator fails, and the report marks it missing.
    let report = run(success_jobs())?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report
            .required_job_results
            .iter()
            .any(|job| job.job_id == "actionlint" && job.conclusion == JobConclusion::Missing),
        "{:?}",
        report.required_job_results
    );

    // An unexpected validator fails as a corrupt evidence set.
    let extra = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
        {"job_id": "velnor-stray", "conclusion": "success"},
    ]);
    let report = run(extra)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"cache_corrupt".to_owned()),
        "{:?}",
        report.miss_reasons
    );

    // A duplicated validator fails the same way.
    let dupe = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
    ]);
    assert_eq!(run(dupe)?.status, FinalStatus::PlanningFailed);

    // Exact coverage passes.
    let full = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
    ]);
    assert_eq!(run(full)?.status, FinalStatus::Passed);
    Ok(())
}

#[test]
fn validator_states_fold_per_validator() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    for failing in ["plan", "actionlint"] {
        for (conclusion, status) in [
            ("failure", FinalStatus::Failed),
            ("cancelled", FinalStatus::Cancelled),
            ("skipped", FinalStatus::NotRun),
        ] {
            let jobs: Vec<serde_json::Value> = ["plan", "actionlint"]
                .iter()
                .map(|id| {
                    let result = if *id == failing {
                        conclusion
                    } else {
                        "success"
                    };
                    json!({"job_id": id, "conclusion": result})
                })
                .collect();
            let plan_value = serde_json::to_value(&plan)?;
            let mut request = merge_request(
                &plan_value,
                &matrix,
                &serde_json::to_value(&reports)?,
                &serde_json::Value::Array(jobs),
            );
            request["required_job_ids"] = json!(["plan", "actionlint"]);
            assert_eq!(status_of(&request)?, status, "{failing}={conclusion}");
        }
    }
    Ok(())
}

#[test]
fn candidate_evidence_is_its_needs_conclusion() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let gate = |request: &mut serde_json::Value, conclusion: &str| {
        request["required_job_ids"] = json!(["candidate", "plan"]);
        request["required_jobs"] = json!([
            {"job_id": "candidate", "conclusion": conclusion},
            {"job_id": "plan", "conclusion": "success"},
        ]);
    };
    let base = || {
        let mut request = merge_request(
            &plan_value,
            &matrix,
            &serde_json::to_value(&reports).expect("reports"),
            &success_jobs(),
        );
        // S3 binds the candidate to the plan head independently of the
        // needs conclusion under test; stage the matching attestation
        // so this test isolates the conclusion dimension.
        request["candidate_attestation"] = serde_json::json!({"schema": 1, "commit": plan.head});
        request
    };

    // A gating candidate with a success conclusion passes: the needs
    // conclusion is the qualification evidence, no report file needed.
    let mut passing = base();
    gate(&mut passing, "success");
    assert_eq!(status_of(&passing)?, FinalStatus::Passed);

    // Failed, cancelled, and skipped qualifications fail the verdict.
    for (conclusion, status) in [
        ("failure", FinalStatus::Failed),
        ("cancelled", FinalStatus::Cancelled),
        ("skipped", FinalStatus::NotRun),
    ] {
        let mut request = base();
        gate(&mut request, conclusion);
        assert_eq!(status_of(&request)?, status, "{conclusion}");
    }

    // A gating candidate without its conclusion fails closed.
    let mut missing = base();
    missing["required_job_ids"] = json!(["candidate", "plan"]);
    missing["required_jobs"] = json!([
        {"job_id": "plan", "conclusion": "success"},
    ]);
    assert_eq!(status_of(&missing)?, FinalStatus::PlanningFailed);
    Ok(())
}

#[test]
fn duplicate_matrix_reports_partition_to_not_run() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    reports.push(reports[0].clone());
    let request = merge_request(
        &serde_json::to_value(&plan)?,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::NotRun);
    assert!(report.miss_reasons.contains(&"cache_corrupt".to_owned()));
    assert_eq!(report.counts.not_run, 2); // matrix plus task duplicates
    Ok(())
}

#[test]
fn missing_report_file_fails_closed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    assert!(reports.len() > 1, "fixture needs two entries");
    let dir = tempfile::TempDir::new()?;
    let run = dir.path().join("run");
    std::fs::create_dir_all(run.join("reports"))?;
    std::fs::write(run.join("plan.json"), serde_json::to_string(&plan)?)?;
    std::fs::write(
        run.join("matrix.json"),
        serde_json::to_string(&plan.matrix)?,
    )?;
    // Stage every report but the first: the missing leg must fail closed.
    for report in &reports[1..] {
        let entry = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.report_id == report.report_id)
            .ok_or_else(|| std::io::Error::other("report without entry"))?;
        let leg = run.join("reports").join(&entry.artifact_id);
        let leg = leg.join(&entry.matrix_key);
        std::fs::create_dir_all(&leg)?;
        std::fs::write(
            leg.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        errors
            .iter()
            .any(|e| e.as_str().is_some_and(|s| s.starts_with("missing_report:"))),
        "{errors:?}"
    );
    value["required_job_ids"] = json!(["plan"]);
    value["required_jobs"] = json!([{"job_id": "plan", "conclusion": "success"}]);
    value["assembly_errors"] = serde_json::Value::Array(
        errors
            .into_iter()
            .filter(|e| e.as_str() != Some("missing_needs_channel"))
            .collect(),
    );
    value["actual_event"] = value["plan"]["event"].clone();
    let report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&value.to_string())?)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"source_missing".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}
#[test]
fn leaf_edit_verifies_unproven_peers() -> TestResult {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    let base = commit(repo.path(), "one")?;
    put(repo.path(), "beta/src/lib.rs", BUMP)?;
    let head = commit(repo.path(), "two")?;
    let (plan, _) = plan_at(repo.path(), Some(&base), &head, None)?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute),
        "no baseline, all execute: {:?}",
        plan.obligations
    );
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &plan_value,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::Passed);
    assert_eq!(report.counts.executed as usize, plan.obligations.len());
    Ok(())
}

#[test]
fn changed_obligations_never_baseline_cover() -> TestResult {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    anchor_repo(repo.path())?;
    let base = commit(repo.path(), "one")?;
    put(repo.path(), "beta/src/lib.rs", BUMP)?;
    let head = commit(repo.path(), "two")?;
    let (seed, _) = plan_at(repo.path(), Some(&base), &head, None)?;
    // The manifest matches every identity exactly, yet changed work must
    // still execute: the changed hint guards incomplete identities.
    let manifest = manifest_for(&seed, &base, &entries_for(&seed));
    let (plan, _) = plan_at(repo.path(), Some(&base), &head, Some(manifest))?;
    for ob in &plan.obligations {
        if ob.task_id.contains("beta") {
            assert_eq!(ob.decision, ObligationDecision::Execute, "{}", ob.task_id);
            assert_eq!(ob.reason, "affected_by_change");
        } else {
            assert_eq!(
                ob.decision,
                ObligationDecision::CoveredByTrustedBaseline,
                "{}",
                ob.task_id
            );
        }
    }
    Ok(())
}

#[test]
fn plan_reuse_decisions_rejected_without_proof() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut plan_json = serde_json::to_value(&plan)?;
    plan_json["obligations"][0]["decision"] = json!("reused_from_task_cache");
    let matrix = plan_json["matrix"].clone();
    let reports = passing_reports(&plan)?;
    let request = merge_request(
        &plan_json,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"no_entry".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

#[test]
fn checkout_mismatch_rejects_plan() -> TestResult {
    let repo = make_repo(crate::impl_common::config_with_branch())?;
    let root = repo.path();
    crate::impl_common::git(&["add", "."], root)?;
    crate::impl_common::git(&["commit", "-m", "one"], root)?;
    let output = git_fixture::command(root)?
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    let base = String::from_utf8(output.stdout)?.trim().to_owned();
    // A head the checkout never reached plans nothing.
    let request = serde_json::json!({"schema": 1, "run_key": "local",
        "base": None::<String>, "head": "f".repeat(40), "event": "push",
        "root": root.display().to_string()});
    let err = velnor_actions_orchestrator::plan_internal(&request.to_string())
        .expect_err("mismatch rejected");
    assert!(err.to_string().contains("checkout_head_mismatch"), "{err}");
    // The checked-out commit itself plans normally.
    let request = serde_json::json!({"schema": 1, "run_key": "local",
        "base": base.clone(), "head": base, "event": "push",
        "root": root.display().to_string()});
    velnor_actions_orchestrator::plan_internal(&request.to_string())?;
    Ok(())
}
