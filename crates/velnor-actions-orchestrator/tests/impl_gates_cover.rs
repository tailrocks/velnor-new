//! Gate 5 cases: baseline misses schedule work; claims revalidate at merge.

use velnor_actions_contract::{FinalStatus, ObligationDecision, Plan};
use velnor_actions_orchestrator::{baseline_artifact_numeric_id, merge_internal, plan_internal};

use crate::impl_common::{
    TestResult, config_with_branch, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};

/// Plan the source-change fixture with an optional baseline manifest value.
pub(crate) fn plan_with_manifest(
    manifest: Option<serde_json::Value>,
) -> Result<(tempfile::TempDir, Plan), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        root,
    )?;
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
    plan_at_with_repository(root, base, head, manifest, None)
}

/// Plan one repo with an explicit runner-owned repository slug.
///
/// The slug is the request capability the planner resolves against
/// the git origin; `None` models a local run (origin fallback).
/// Ambient process env never participates, so these plans are
/// identical under any runner environment.
pub(crate) fn plan_at_with_repository(
    root: &std::path::Path,
    base: &str,
    head: &str,
    manifest: Option<serde_json::Value>,
    repository: Option<&str>,
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
    if let Some(repository) = repository {
        request["repository"] = serde_json::Value::String(repository.to_owned());
    }
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    Ok(serde_json::from_value(value["plan"].clone())?)
}

/// Minimal manifest JSON for one base commit; tasks filled by callers.
pub(crate) fn manifest_for(
    plan: &Plan,
    base: &str,
    tasks: &serde_json::Value,
) -> serde_json::Value {
    let compat = velnor_actions_contract::digest_b3(b"compat");
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let name = format!("velnor-baseline-{base}-{compat}");
    serde_json::json!({
        "schema": 2,
        "repository_id": velnor_actions_contract::digest_b3(b"github.com/o/r"),
        "source_commit": base,
        "ref": "refs/heads/testmain",
        "event": "push",
        "workflow_ref": format!("o/r/{workflow}@refs/heads/testmain"),
        "run_id": 7,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": plan.generator.version,
        "generator_sha256": plan.generator.sha256,
        "compatibility_id": compat,
        "artifact_id": baseline_artifact_numeric_id(&name),
        "parent": null,
        "artifact_name": name,
        "tasks": tasks,
        "parent": serde_json::Value::Null,
    })
}

/// Task entries binding every plan obligation exactly.
pub(crate) fn entries_for(plan: &Plan) -> serde_json::Value {
    let tasks: Vec<serde_json::Value> = plan
        .obligations
        .iter()
        .map(|ob| {
            serde_json::json!({
                "task_id": ob.task_id,
                "task_digest": ob.task_digest,
                "input_digest": ob.input_digest,
                "closure_digest": ob.closure_digest,
                "proof_run_id": 7,
                "carried_from": null,
                "observed_run_id": 7,
                "carried_from": serde_json::Value::Null,
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
        plan.baseline.status(),
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
    stale["schema"] = serde_json::json!(1);
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
    let head = seed.head.clone();
    let mut tasks = entries_for(&seed);
    let tampered_id = tasks[0]["task_id"].as_str().expect("task id").to_owned();
    tasks[0]["input_digest"] =
        serde_json::Value::String(velnor_actions_contract::digest_b3(b"tampered"));
    let manifest = manifest_for(&seed, &head, &tasks);
    let plan = plan_at(repo.path(), &head, &head, Some(manifest))?;
    assert!(plan.obligations.len() > 1, "needs covered + tampered pair");
    for ob in &plan.obligations {
        if ob.task_id == tampered_id {
            assert_eq!(ob.decision, ObligationDecision::Execute, "{ob:?}");
            assert!(ob.baseline_proof.is_none(), "{ob:?}");
        } else {
            assert_eq!(
                ob.decision,
                ObligationDecision::CoveredByTrustedBaseline,
                "{ob:?}"
            );
            assert!(ob.baseline_proof.is_some(), "{ob:?}");
        }
    }
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("baseline_miss:") && w.contains("no_entry")),
        "{:?}",
        plan.warnings
    );
    assert_eq!(
        plan.baseline.status(),
        velnor_actions_contract::BaselineStatus::Used
    );
    Ok(())
}

#[test]
fn valid_manifest_covers_exact_obligations() -> TestResult {
    let (repo, seed) = plan_with_manifest(None)?;
    assert!(!seed.obligations.is_empty());
    let head = seed.head.clone();
    let manifest = manifest_for(&seed, &head, &entries_for(&seed));
    let artifact_name = manifest["artifact_name"]
        .as_str()
        .expect("artifact name")
        .to_owned();
    let plan = plan_at(repo.path(), &head, &head, Some(manifest))?;
    for ob in &plan.obligations {
        assert_eq!(
            ob.decision,
            ObligationDecision::CoveredByTrustedBaseline,
            "{ob:?}"
        );
        let proof = ob.baseline_proof.as_ref().expect("proof");
        assert_eq!(proof.source_commit(), head);
        assert_eq!(
            proof.artifact_id(),
            baseline_artifact_numeric_id(&artifact_name)
        );
        assert_eq!(proof.artifact_name(), artifact_name);
    }
    assert!(plan.matrix.include.is_empty(), "{:?}", plan.matrix.include);
    assert!(
        !plan
            .warnings
            .iter()
            .any(|w| w.contains("plan_invalid:reverted")),
        "{:?}",
        plan.warnings
    );
    plan.validate()?;
    assert_eq!(
        plan.baseline.status(),
        velnor_actions_contract::BaselineStatus::Used
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
    let proof_base = plan.base.clone().expect("base");
    plan.obligations[first].baseline_proof = Some(velnor_actions_contract::BaselineProof::new(
        &proof_base,
        7,
        9,
        "velnor-plan-local",
        &velnor_actions_contract::digest_b3(b"manifest"),
    )?);
    plan.validate()?;
    let reports = passing_reports(&plan)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": reports,
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
    });
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&request.to_string())?)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    Ok(())
}

#[test]
fn merge_group_classifies_like_pull_request() -> TestResult {
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
    assert_eq!(pr.trust, velnor_actions_contract::Trust::Pr);
    assert_eq!(
        group.trust,
        velnor_actions_contract::Trust::Pr,
        "speculative merge content stays PR-scoped"
    );
    let push = plan_for("push")?;
    assert_eq!(push.trust, velnor_actions_contract::Trust::Trusted);
    Ok(())
}
