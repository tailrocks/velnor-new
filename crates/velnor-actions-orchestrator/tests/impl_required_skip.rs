//! Required gate distinguishes deliberate coverage from absent execution.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use velnor_actions_contract::{FinalStatus, ObligationDecision, Plan, is_crate_job_id};

use crate::impl_common::{TestResult, passing_reports, without_ambient_ci_env};
use crate::impl_orch_core::{merge, merge_request};
use crate::impl_orch_plansel::{
    BUMP, anchor_repo, commit, entries_for, make_ws, manifest_for, plan_at, put,
};

/// Genuine baseline and unchanged/changed two-crate plan, plus its request.
fn covered_request(changed: bool) -> Result<(Plan, Value), Box<dyn std::error::Error>> {
    let repo = make_ws(&["alpha", "beta"], &[])?;
    anchor_repo(repo.path())?;
    let base = commit(repo.path(), "base")?;
    let head = if changed {
        put(repo.path(), "beta/src/lib.rs", BUMP)?;
        commit(repo.path(), "candidate")?
    } else {
        base.clone()
    };
    let (seed, _) = plan_at(repo.path(), Some(&base), &head, None)?;
    let manifest = manifest_for(&seed, &base, &entries_for(&seed));
    let (plan, _) = plan_at(repo.path(), Some(&base), &head, Some(manifest.clone()))?;
    let mut jobs: BTreeSet<_> = plan
        .obligations
        .iter()
        .map(|ob| ob.job_id.as_str())
        .collect();
    jobs.insert("plan");
    let jobs: Vec<_> = jobs
        .into_iter()
        .map(|id| {
            let covered = is_crate_job_id(id)
                && plan
                    .obligations
                    .iter()
                    .filter(|ob| ob.job_id == id)
                    .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline);
            json!({"job_id": id, "conclusion": if covered {"skipped"} else {"success"}})
        })
        .collect();
    let mut request = merge_request(
        &serde_json::to_value(&plan)?,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(passing_reports(&plan)?)?,
        &json!(jobs),
    );
    request["baseline_manifest"] = manifest;
    Ok((plan, request))
}

/// Rewrite one validator's conclusion without changing its declaration.
fn set_conclusion(request: &mut Value, job_id: &str, conclusion: &str) -> TestResult {
    let job = request["required_jobs"]
        .as_array_mut()
        .ok_or("jobs")?
        .iter_mut()
        .find(|job| job["job_id"] == job_id)
        .ok_or("job")?;
    job["conclusion"] = json!(conclusion);
    Ok(())
}

#[test]
fn fully_covered_crate_jobs_skip_without_reports() -> TestResult {
    without_ambient_ci_env("fully_covered_crate_jobs_skip_without_reports", || {
        let (plan, request) = covered_request(false)?;
        assert!(plan.matrix.include.is_empty());
        assert!(!plan.obligations.is_empty());
        assert!(
            request["required_jobs"]
                .as_array()
                .ok_or("jobs")?
                .iter()
                .any(|job| { job["conclusion"] == "skipped" })
        );
        let report = merge(&request)?;
        assert_eq!(report.status, FinalStatus::Passed);
        assert_eq!(report.counts.covered as usize, plan.obligations.len());
        assert_eq!(report.counts.executed, 0);
        assert_eq!(report.counts.not_run, 0);
        Ok(())
    })
}

#[test]
fn covered_job_skip_never_excuses_bad_conclusions() -> TestResult {
    without_ambient_ci_env("covered_job_skip_never_excuses_bad_conclusions", || {
        let (plan, request) = covered_request(false)?;
        let job = &plan.obligations.first().ok_or("obligation")?.job_id;
        for (conclusion, status) in [
            ("failure", FinalStatus::Failed),
            ("cancelled", FinalStatus::Cancelled),
            ("neutral", FinalStatus::NotRun),
            ("missing", FinalStatus::Failed),
        ] {
            let mut changed = request.clone();
            set_conclusion(&mut changed, job, conclusion)?;
            assert_eq!(merge(&changed)?.status, status, "{conclusion}");
        }
        let mut missing = request;
        missing["required_jobs"]
            .as_array_mut()
            .ok_or("jobs")?
            .retain(|row| row["job_id"] != *job);
        assert_eq!(merge(&missing)?.status, FinalStatus::PlanningFailed);
        Ok(())
    })
}

#[test]
fn skipped_job_requires_nonempty_bound_crate_obligations() -> TestResult {
    without_ambient_ci_env(
        "skipped_job_requires_nonempty_bound_crate_obligations",
        || {
            let (_, request) = covered_request(false)?;
            for job_id in ["actionlint", "plan", "rust-unbound"] {
                let mut changed = request.clone();
                if job_id == "plan" {
                    set_conclusion(&mut changed, job_id, "skipped")?;
                    assert_eq!(merge(&changed)?.status, FinalStatus::NotRun, "{job_id}");
                    continue;
                }
                changed["required_job_ids"]
                    .as_array_mut()
                    .ok_or("inventory")?
                    .push(json!(job_id));
                changed["required_jobs"]
                    .as_array_mut()
                    .ok_or("jobs")?
                    .push(json!({"job_id": job_id, "conclusion": "skipped"}));
                assert_eq!(merge(&changed)?.status, FinalStatus::NotRun, "{job_id}");
            }
            Ok(())
        },
    )
}

#[test]
fn covered_job_skip_revalidates_baseline_proof() -> TestResult {
    without_ambient_ci_env("covered_job_skip_revalidates_baseline_proof", || {
        let (_, request) = covered_request(false)?;
        let mut missing = request.clone();
        missing["baseline_manifest"] = Value::Null;
        assert_eq!(merge(&missing)?.status, FinalStatus::PlanningFailed);
        for (field, value) in [
            ("final_status", json!("failed")),
            ("expires_at_unix", json!(1)),
            ("source_commit", json!("f".repeat(40))),
            ("generator_sha256", json!("f".repeat(64))),
        ] {
            let mut changed = request.clone();
            changed["baseline_manifest"][field] = value;
            assert_eq!(
                merge(&changed)?.status,
                FinalStatus::PlanningFailed,
                "{field}"
            );
        }
        let mut incomplete = request;
        incomplete["baseline_manifest"]["tasks"]
            .as_array_mut()
            .ok_or("tasks")?
            .pop();
        assert_eq!(merge(&incomplete)?.status, FinalStatus::PlanningFailed);
        Ok(())
    })
}

#[test]
fn covered_obligation_owner_must_be_declared_in_needs() -> TestResult {
    without_ambient_ci_env("covered_obligation_owner_must_be_declared_in_needs", || {
        let (plan, mut request) = covered_request(false)?;
        let job_id = &plan.obligations.first().ok_or("obligation")?.job_id;
        request["required_job_ids"]
            .as_array_mut()
            .ok_or("inventory")?
            .retain(|id| id != job_id);
        request["required_jobs"]
            .as_array_mut()
            .ok_or("jobs")?
            .retain(|job| job["job_id"] != *job_id);
        let report = merge(&request)?;
        assert_eq!(report.status, FinalStatus::PlanningFailed);
        assert!(report.miss_reasons.contains(&"no_entry".to_owned()));
        Ok(())
    })
}

#[test]
fn covered_peer_never_excuses_selected_job_or_report() -> TestResult {
    without_ambient_ci_env("covered_peer_never_excuses_selected_job_or_report", || {
        let (plan, request) = covered_request(true)?;
        assert!(!plan.matrix.include.is_empty());
        let job_id = &plan.matrix.include.first().ok_or("selected")?.job_id;
        assert!(
            plan.obligations
                .iter()
                .any(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
        );
        assert_eq!(merge(&request)?.status, FinalStatus::Passed);
        for (conclusion, status) in [
            ("skipped", FinalStatus::NotRun),
            ("cancelled", FinalStatus::Cancelled),
            ("failure", FinalStatus::Failed),
        ] {
            let mut changed = request.clone();
            set_conclusion(&mut changed, job_id, conclusion)?;
            assert_eq!(merge(&changed)?.status, status, "{conclusion}");
        }
        for field in ["matrix_reports", "task_reports"] {
            let mut missing = request.clone();
            missing[field] = json!([]);
            assert_eq!(merge(&missing)?.status, FinalStatus::NotRun, "{field}");
        }
        Ok(())
    })
}
