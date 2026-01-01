//! T23: tofu init/validate task-result reuse is OFF; a provider-cache
//! hit still runs selected validation (`executed`, never `reused`).
//!
//! fmt keeps the shared qualification (the T23 row scopes OFF to
//! init/validate); rust reuse is untouched.
use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, FinalStatus, ObligationDecision, Plan, TaskReport,
    TaskStatus, Trust, WorkflowEvent, task_report_id_for_task,
};
use velnor_actions_orchestrator::plan_internal;

use super::impl_common::{TestResult, git, git_line, passing_reports};
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

/// Unchanged tofu plans init/validate as reuse-refused, never reusable:
/// both carry `task_not_eligible` while fmt still passes the identity
/// gate (`forced_uncached`, like every other eligible task).
#[test]
fn unchanged_tofu_init_validate_refuse_reuse() -> TestResult {
    let (_dir, plan) = plan_for_unchanged_tofu()?;
    assert_eq!(plan.obligations.len(), 6, "two roots x triple");
    for obligation in &plan.obligations {
        assert_eq!(
            obligation.decision,
            ObligationDecision::Execute,
            "{} never reuses",
            obligation.task_id
        );
        let kind = obligation
            .task_id
            .rsplit('/')
            .nth(1)
            .ok_or("kind segment")?;
        let expected = match kind {
            "init" | "validate" => "task_not_eligible",
            "fmt" => "forced_uncached",
            _ => return Err(format!("unknown tofu kind: {kind}").into()),
        };
        assert_eq!(
            obligation.reason, expected,
            "{} carries its T23 reason",
            obligation.task_id
        );
    }
    Ok(())
}

/// A provider-cache hit verifies, and the validate leg still reports
/// `executed` alongside the hit — then merges green.
#[test]
fn validate_reports_executed_after_provider_cache_hit() -> TestResult {
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
    let obligation = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == "stack/tofu/stacks/a/validate/default")
        .ok_or("validate obligation")?;
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    let entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id == obligation.task_id)
        .ok_or("validate matrix entry")?;
    let report = TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task(
            "local",
            &entry.matrix_key,
            &obligation.task_digest,
        )?,
        run_key: "local".to_owned(),
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: obligation.task_id.clone(),
        task_digest: obligation.task_digest.clone(),
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::TofuProviders,
            key: "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-stacks-a-0123456789ab-${{hashFiles('stacks/a/.terraform.lock.hcl')}}"
                .to_owned(),
            result: CacheResult::Hit,
            miss_reason: None,
        },
        exit_code: 0,
        duration_ms: None,
        outputs: vec![],
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    let value = serde_json::to_value(&report)?;
    assert_eq!(value["status"], json!("executed"));
    assert_eq!(value["cache"]["layer"], json!("tofu-providers"));
    assert_eq!(value["cache"]["result"], json!("hit"));
    assert!(value["cache"].get("miss_reason").is_none());
    let reports = passing_reports(&plan)?;
    assert!(
        reports.iter().all(|report| report.executed == 1),
        "every leg executes"
    );
    let request = merge_request(
        &serde_json::to_value(&plan)?,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    Ok(())
}
