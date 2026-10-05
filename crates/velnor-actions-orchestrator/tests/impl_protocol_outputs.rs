//! Plan-output protocol and dispatch policy cases.
use std::fs;

use velnor_actions_contract::canonical_json_str;
use velnor_actions_orchestrator::{PlanOutputMode, plan_internal, plan_outputs};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, plan_for_source_change,
};
use crate::impl_orch_core_cover::covered_plan;

#[test]
fn plan_outputs_agree_with_plan_matrix() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(outputs.matrix, canonical_json_str(&value["matrix"])?);
    assert!(!outputs.matrix.contains('\n'));
    assert!(
        outputs.covered_tasks.is_empty(),
        "execute-all plans emit no channel"
    );
    assert!(
        err_of(
            plan_outputs("not json", PlanOutputMode::Static),
            "outputs reject garbage"
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn plan_outputs_encode_covered_tasks() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.task_ids.is_empty(), "fixture must select work");
    let (plan_json, _) = covered_plan(&plan)?;
    let response = serde_json::json!({
        "schema": 1,
        "plan": plan_json,
        "matrix": plan_json["matrix"],
    });
    let outputs = plan_outputs(&response.to_string(), PlanOutputMode::Static)?;
    let mut ids: Vec<&str> = plan
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(outputs.covered_tasks, format!(",{},", ids.join(",")));
    Ok(())
}

#[test]
fn plan_outputs_bind_qualification_phase_and_cache_policy() -> TestResult {
    let (repo, _) = plan_for_source_change()?;
    let root = repo.path();
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let context = serde_json::json!({
        "campaign": "protocol-test",
        "phase": "third",
        "repository": "owner/project",
        "default_branch": "testmain",
        "git_ref": "refs/heads/testmain",
        "ref_protected": true,
        "workflow_ref": "owner/project/.github/workflows/ci.yml@refs/heads/testmain",
        "workflow_sha": head,
        "source_sha": head,
        "run_id": 7,
        "run_attempt": 2,
        "predecessor": { "run_id": 6, "run_attempt": 1 },
    });
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "r7-a2",
        "base": null,
        "head": head,
        "event": "qualification",
        "qualification": context,
        "root": root.display().to_string(),
        "repository": "owner/project",
    });
    let response = plan_internal(&request.to_string())?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    assert_eq!(outputs.qualification_campaign, "protocol-test");
    assert_eq!(outputs.qualification_phase, "third");
    assert!(outputs.qualification_cache_enabled);
    assert!(!outputs.qualification_cache_write);
    Ok(())
}
