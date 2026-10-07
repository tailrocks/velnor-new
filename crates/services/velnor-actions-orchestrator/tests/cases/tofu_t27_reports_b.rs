//! T27 tofu propagation and event cases: shared-module changes ride
//! plan to a green merge, and pull-request versus merged-main runs
//! stamp their event and trust at the plan level.
//!
//! A change inside a module shared by one root selects exactly that
//! root's triple end to end. PR plans stamp `pull_request`/`pr`,
//! merged-main pushes stamp `push`/`trusted`, and a merge under the
//! wrong event fails trust coherence closed.
use std::fs;
use std::path::Path;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract_workflow::{FinalStatus, Plan, Trust, WorkflowEvent};
use velnor_actions_orchestrator::plan_internal;

use crate::cases::orch_core::{merge, merge_request, success_jobs};
use crate::impl_select::{commit, plan_pr, plan_push, reasons_for};
use crate::support::{
    TestResult, git, git_line, install_fixture_release_manifest, passing_reports,
};

/// Git-initialized pure-tofu repo: `config` plus `files`, no Cargo.
fn make_pure_tofu_repo(
    config: &str,
    files: &[(&str, &str)],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    install_fixture_release_manifest(root)?;
    for (relative, content) in files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    Ok(dir)
}

/// Two-root tofu config over `stacks/a` and `stacks/b`.
fn two_root_config() -> String {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\"]\n"
        .to_owned()
}

/// Two roots where `stacks/a` calls the shared module `./mods/m`.
fn module_tree() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "stacks/a/main.tf",
            "module \"m\" {\n  source = \"./mods/m\"\n}\n",
        ),
        ("stacks/a/mods/m/main.tf", "variable \"x\" {}\n"),
        ("stacks/b/main.tf", "variable \"y\" {}\n"),
    ]
}

/// Merge one tofu plan with passing reports and a passing plan job.
fn merge_passing(plan: &Plan) -> Result<FinalStatus, Box<dyn std::error::Error>> {
    let reports = passing_reports(plan)?;
    let plan_value = serde_json::to_value(plan)?;
    let request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    Ok(merge(&request)?.status)
}

/// A shared-module change selects its caller and merges green.
#[test]
fn tofu_shared_module_change_rides_plan_to_passed() -> TestResult {
    let dir = make_pure_tofu_repo(&two_root_config(), &module_tree())?;
    let root = dir.path();
    let base = commit(root, "base")?;
    fs::write(
        root.join("stacks/a/mods/m/main.tf"),
        "variable \"x\" {}\nvariable \"z\" {}\n",
    )?;
    let head = commit(root, "head")?;
    let (plan, _) = plan_pr(root, Some(&base), &head)?;
    assert_eq!(plan.matrix.include.len(), 6, "full universe planned");
    for obligation in &plan.obligations {
        let affected = obligation.task_id.starts_with("stack/tofu/stacks/a/");
        assert_eq!(
            obligation.reason == "affected_by_change",
            affected,
            "{}: {}",
            obligation.task_id,
            obligation.reason
        );
    }
    let hit: Vec<&str> = reasons_for(&plan, "stacks/a");
    assert_eq!(hit.len(), 3, "calling triple propagates: {hit:?}");
    assert_eq!(merge_passing(&plan)?, FinalStatus::Passed);
    Ok(())
}

/// A pull-request plan stamps the PR event and trust over tofu work.
#[test]
fn tofu_pr_plan_stamps_pr_event_and_trust() -> TestResult {
    let dir = make_pure_tofu_repo(
        &two_root_config(),
        &[
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
            ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ],
    )?;
    let root = dir.path();
    let base = commit(root, "base")?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"bump\" {}\n")?;
    let head = commit(root, "head")?;
    let (plan, _) = plan_pr(root, Some(&base), &head)?;
    assert_eq!(plan.event, WorkflowEvent::PullRequest);
    assert_eq!(plan.trust, Trust::Pr);
    assert!(!plan.obligations.is_empty(), "PR plans tofu work");
    assert!(
        plan.obligations
            .iter()
            .any(|ob| ob.reason == "affected_by_change"),
        "changed root marks affected"
    );
    assert_eq!(merge_passing(&plan)?, FinalStatus::Passed);
    Ok(())
}

/// A merged-main push stamps the push event and trusted scope.
#[test]
fn tofu_push_plan_stamps_push_event_and_trust() -> TestResult {
    let dir = make_pure_tofu_repo(
        &two_root_config(),
        &[
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
            ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ],
    )?;
    let root: &Path = dir.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "merged"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "base": serde_json::Value::Null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    assert_eq!(plan.event, WorkflowEvent::Push);
    assert_eq!(plan.trust, Trust::Trusted);
    assert!(!plan.obligations.is_empty(), "push plans tofu work");
    assert_eq!(merge_passing(&plan)?, FinalStatus::Passed);
    Ok(())
}

/// Merging a PR plan under the push event fails trust coherence.
#[test]
fn tofu_event_mismatch_fails_required_planning_failed() -> TestResult {
    let dir = make_pure_tofu_repo(
        &two_root_config(),
        &[
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
            ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ],
    )?;
    let root = dir.path();
    let base = commit(root, "base")?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"bump\" {}\n")?;
    let head = commit(root, "head")?;
    let (plan, _) = plan_pr(root, Some(&base), &head)?;
    let (pushed, _) = plan_push(root, Some(&base), &head)?;
    assert_eq!(pushed.event, WorkflowEvent::Push);
    assert_eq!(pushed.trust, Trust::Trusted);
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let mut request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    request["actual_event"] = json!("push");
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}
