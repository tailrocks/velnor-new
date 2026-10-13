//! Gate 5 cases: baseline misses schedule work; claims revalidate at merge.

use velnor_actions_contract::{FinalStatus, ObligationDecision, Plan};
use velnor_actions_orchestrator::{merge_internal, plan_internal};

use crate::impl_common::{
    TestResult, config_with_branch, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};

/// Plan the source-change fixture with an optional baseline manifest value.
fn plan_with_manifest(
    manifest: Option<serde_json::Value>,
) -> Result<(tempfile::TempDir, Plan), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan = plan_at(root, &base, &head, manifest)?;
    Ok((repo, plan))
}

/// Plan one repo at explicit revisions with an optional manifest value.
fn plan_at(
    root: &std::path::Path,
    base: &str,
    head: &str,
    manifest: Option<serde_json::Value>,
) -> Result<Plan, Box<dyn std::error::Error>> {
    let mut request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    if let Some(manifest) = manifest {
        request["baseline_manifest"] = manifest;
    }
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    Ok(serde_json::from_value(value["plan"].clone())?)
}

/// Minimal manifest JSON for one base commit; tasks filled by callers.
fn manifest_for(plan: &Plan, base: &str, tasks: &serde_json::Value) -> serde_json::Value {
    let compat = velnor_actions_contract::digest_b3(b"compat");
    serde_json::json!({
        "schema": 1,
        "repository_id": velnor_actions_contract::digest_b3(b"repo"),
        "source_commit": base,
        "ref": "refs/heads/testmain",
        "event": "push",
        "workflow_ref": "o/r/.github/workflows/velnor.yml@refs/heads/testmain",
        "run_id": 7,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": plan.generator.version,
        "generator_sha256": plan.generator.sha256,
        "compatibility_id": compat,
        "artifact_id": 9,
        "artifact_name": format!("velnor-baseline-{base}-{compat}"),
        "tasks": tasks,
    })
}

/// Task entries binding every plan obligation exactly.
fn entries_for(plan: &Plan) -> serde_json::Value {
    let tasks: Vec<serde_json::Value> = plan
        .obligations
        .iter()
        .map(|ob| {
            serde_json::json!({
                "task_id": ob.task_id,
                "task_digest": ob.task_digest,
                "input_digest": ob.input_digest,
                "proof_run_id": 7,
                "observed_run_id": 7,
            })
        })
        .collect();
    serde_json::Value::Array(tasks)
}

#[test]
fn wrong_base_manifest_schedules_everything() -> TestResult {
    let (_repo, seed) = plan_for_source_change()?;
    assert!(!seed.task_ids.is_empty());
    let base = seed.base.clone().expect("base");
    let wrong = "b".repeat(40);
    assert_ne!(wrong, base);
    let manifest = manifest_for(&seed, &wrong, &entries_for(&seed));
    let (_repo, plan) = plan_with_manifest(Some(manifest))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert_eq!(plan.matrix.include.len(), seed.matrix.include.len());
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("baseline_miss:wrong_commit")),
        "{:?}",
        plan.warnings
    );
    assert_eq!(
        plan.baseline.status,
        velnor_actions_contract::BaselineStatus::Unavailable
    );
    Ok(())
}

#[test]
fn malformed_manifest_is_a_miss_not_a_failure() -> TestResult {
    let (_repo, _seed) = plan_for_source_change()?;
    let malformed = serde_json::json!({"schema": "one"});
    let (_repo, plan) = plan_with_manifest(Some(malformed))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("baseline_miss:malformed_manifest")),
        "{:?}",
        plan.warnings
    );
    let (repo, seed) = plan_with_manifest(None)?;
    let base = seed.base.clone().expect("base");
    let mut stale = manifest_for(&seed, &base, &entries_for(&seed));
    stale["schema"] = serde_json::json!(2);
    let plan = plan_at(repo.path(), &base, &seed.head, Some(stale))?;
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("baseline_miss:stale_schema")),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn tampered_task_entry_executes_with_miss_warning() -> TestResult {
    let (repo, seed) = plan_with_manifest(None)?;
    let base = seed.base.clone().expect("base");
    let mut tasks = entries_for(&seed);
    tasks[0]["input_digest"] =
        serde_json::Value::String(velnor_actions_contract::digest_b3(b"tampered"));
    let manifest = manifest_for(&seed, &base, &tasks);
    let plan = plan_at(repo.path(), &base, &seed.head, Some(manifest))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("baseline_miss:") && w.contains("no_entry")),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn valid_manifest_reverts_safely_until_contract_accepts_baseline_names() -> TestResult {
    let (repo, seed) = plan_with_manifest(None)?;
    let base = seed.base.clone().expect("base");
    let manifest = manifest_for(&seed, &base, &entries_for(&seed));
    let plan = plan_at(repo.path(), &base, &seed.head, Some(manifest))?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert_eq!(plan.matrix.include.len(), seed.matrix.include.len());
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("plan_invalid:reverted")),
        "{:?}",
        plan.warnings
    );
    assert_eq!(
        plan.baseline.status,
        velnor_actions_contract::BaselineStatus::Unavailable
    );
    Ok(())
}

#[test]
fn pr_plan_records_publish_forbidden() -> TestResult {
    let (repo, seed) = plan_with_manifest(None)?;
    let base = seed.base.clone().expect("base");
    let manifest = manifest_for(&seed, &base, &entries_for(&seed));
    let plan = plan_at(repo.path(), &base, &seed.head, Some(manifest))?;
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("baseline_publish:forbidden")),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn merge_rejects_covered_claims_without_manifest() -> TestResult {
    let (_repo, mut plan) = plan_for_source_change()?;
    let first = 0;
    plan.obligations[first].decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.obligations[first].reason = "covered_by_trusted_baseline".to_owned();
    plan.obligations[first].baseline_proof = Some(velnor_actions_contract::BaselineProof {
        source_commit: plan.base.clone().expect("base"),
        run_id: 7,
        artifact_id: 9,
        artifact_name: "velnor-plan-local".to_owned(),
        manifest_digest: velnor_actions_contract::digest_b3(b"manifest"),
    });
    plan.validate()?;
    let reports = passing_reports(&plan)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": reports,
        "required_jobs": [{"job_id": "velnor-plan", "conclusion": "success"}],
    });
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&request.to_string())?)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    Ok(())
}

#[test]
fn merge_group_narrows_like_pull_request() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan_for = |event: &str| -> Result<Plan, Box<dyn std::error::Error>> {
        let request = serde_json::json!({
            "schema": 1,
            "run_key": "local",
            "base": base,
            "head": head,
            "event": event,
            "root": root.display().to_string(),
        });
        let response = plan_internal(&request.to_string())?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        Ok(serde_json::from_value(value["plan"].clone())?)
    };
    let pr = plan_for("pull_request")?;
    let group = plan_for("merge_group")?;
    assert_eq!(pr.task_ids, group.task_ids);
    assert!(
        !group.task_ids.is_empty(),
        "merge group selects affected work"
    );
    Ok(())
}
