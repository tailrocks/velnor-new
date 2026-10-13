//! T21 provider-cache end to end: fail-closed reuse claims,
//! hit-still-runs planning, and one elected saver per root key.
//!
//! Provider entries accelerate init; they never replace validate
//! execution and never mint reuse.
use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract_workflow::{FinalStatus, ObligationDecision, Plan, StepKind};
use velnor_actions_orchestrator::{finalized_jobs, plan_internal, prepare};

use super::impl_common::{
    TestResult, git, git_line, install_fixture_release_manifest, passing_reports,
};
use super::impl_orch_core::{merge, merge_request, success_jobs};

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

fn two_root_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("stacks/a/main.tf", "variable \"a\" {}\n"),
        ("stacks/b/main.tf", "variable \"b\" {}\n"),
    ]
}

/// Plan for a two-commit pure-tofu repo whose second commit is empty.
fn plan_for_unchanged_tofu() -> Result<(TempDir, Plan), Box<dyn std::error::Error>> {
    let dir = make_pure_tofu_repo(&two_root_config(), &two_root_files())?;
    let root = dir.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    git(&["commit", "--allow-empty", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((dir, plan))
}

/// A provider-cache hit verifies, yet init+validate still execute:
/// warmth never mints reuse.
#[test]
fn provider_hit_still_runs_init_and_validate() -> TestResult {
    use velnor_actions_contract::digest_b3;
    use velnor_actions_mise::restore_evidence::{RestoreObservation, verify_provider_restore};
    let bytes = b"provider bytes".to_vec();
    let hit = RestoreObservation {
        entry_path: "tofu-cache/root-0123456789ab/provider".to_owned(),
        entry_bytes: bytes.clone(),
        expected_digest: digest_b3(&bytes),
        expected_compat: digest_b3(b"compat"),
        observed_compat: digest_b3(b"compat"),
        expected_owner: "trusted".to_owned(),
        observed_owner: "trusted".to_owned(),
        expected_inputs: digest_b3(b"inputs"),
        observed_inputs: digest_b3(b"inputs"),
    };
    assert_eq!(verify_provider_restore(&hit), Ok(()), "cache hits");
    let (_dir, plan) = plan_for_unchanged_tofu()?;
    assert!(!plan.obligations.is_empty(), "tofu plans obligations");
    for obligation in &plan.obligations {
        assert_eq!(
            obligation.decision,
            ObligationDecision::Execute,
            "{} still runs despite the hit",
            obligation.task_id
        );
    }
    Ok(())
}

/// A tofu reuse claim fails closed at merge, like every other stack.
#[test]
fn tofu_reuse_claims_fail_closed_at_merge() -> TestResult {
    let (_dir, plan) = plan_for_unchanged_tofu()?;
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

/// Every finalized tofu job saves exactly its own restored key under
/// the push-only gate; the plan job saves nothing.
#[test]
fn finalized_tofu_jobs_save_exactly_their_restored_key() -> TestResult {
    use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
    let dir = make_pure_tofu_repo(&two_root_config(), &two_root_files())?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let plan = jobs.get("plan").ok_or("plan job")?;
    assert!(
        plan.steps
            .iter()
            .all(|step| step.name != "Save Tofu providers"),
        "the plan job never inits so never saves"
    );
    let mut keys = Vec::new();
    for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
        let restore = job
            .steps
            .iter()
            .find(|step| step.name == "Restore Tofu providers")
            .ok_or(format!("{id} restores providers"))?;
        let StepKind::Action { with, .. } = &restore.kind else {
            return Err(format!("{id} restore must be an action step").into());
        };
        let key = with.get("cache-key").ok_or("restore key")?.clone();
        let path = with.get("cache-path").ok_or("restore path")?;
        assert!(
            velnor_actions_workflow_renderer::tofu_cache::tofu_providers_path_ok(path),
            "{id} restores one owned plugin-cache leaf"
        );
        let saves: Vec<_> = job
            .steps
            .iter()
            .filter(|step| step.name == "Save Tofu providers")
            .collect();
        assert_eq!(saves.len(), 1, "{id} saves once");
        let save = saves[0];
        assert_eq!(save.condition.as_deref(), Some(CACHE_SAVE_CONDITION));
        let StepKind::Action { with: inputs, .. } = &save.kind else {
            return Err(format!("{id} save must be an action step").into());
        };
        assert_eq!(
            inputs.get("key").map(String::as_str),
            Some(velnor_actions_contract_workflow::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR),
            "{id} saves the output key from its own restore"
        );
        assert_eq!(
            inputs.get("path").map(String::as_str),
            Some(velnor_actions_contract_workflow::workflow::step_identity::TOFU_PROVIDERS_PATH_OUTPUT_EXPR),
            "{id} saves the output path from its own restore"
        );
        keys.push(key);
    }
    assert_eq!(keys.len(), 2, "one tofu job per root");
    assert_ne!(keys[0], keys[1], "per-root keys stay distinct");
    Ok(())
}
