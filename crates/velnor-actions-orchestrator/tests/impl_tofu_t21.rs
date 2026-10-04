//! T21 provider-cache end to end: fail-closed reuse claims,
//! hit-still-runs planning, and one pure producer per root key.
//!
//! Provider entries accelerate init; they never replace validate
//! execution and never mint reuse.
use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::{
    FinalStatus, Job, ObligationDecision, Plan, SourceProducerRole, StepKind,
};
use velnor_actions_orchestrator::{finalized_jobs, plan_internal, prepare};

use super::impl_common::{TestResult, git, git_line, passing_reports};
use super::impl_orch_core::{merge, merge_request, success_jobs};

/// A complete public-registry lock used by positive provider-cache fixtures.
const PUBLIC_PROVIDER_LOCK: &str = r#"provider "registry.opentofu.org/hashicorp/null" {
  version = "3.2.1"
  hashes = ["h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]
}
"#;

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
        ("stacks/a/.terraform.lock.hcl", PUBLIC_PROVIDER_LOCK),
        ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ("stacks/b/.terraform.lock.hcl", PUBLIC_PROVIDER_LOCK),
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
        entry_path: format!(
            "tofu-cache/{}/provider",
            velnor_actions_tofu::tofu_root_locator("").expect("root locator")
        ),
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
        &success_jobs(&plan),
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

/// Consumers restore read-only; one pure producer saves each public key.
#[test]
fn finalized_tofu_jobs_use_pure_producers_for_provider_saves() -> TestResult {
    let dir = make_pure_tofu_repo(&two_root_config(), &two_root_files())?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let plan = jobs.get("plan").ok_or("plan job")?;
    assert!(
        plan.steps
            .iter()
            .all(|step| step.name != "Save Tofu providers"),
        "the plan job never inits so never saves"
    );
    let mut consumer_keys = Vec::new();
    let mut producer_keys = Vec::new();
    let producer_ids: Vec<&String> = jobs
        .iter()
        .filter(|(_, job)| job.source_producer.is_some())
        .map(|(id, _)| id)
        .collect();
    for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
        if job.source_producer.is_some() {
            let guard = jobs
                .iter()
                .find(|(_, consumer)| {
                    consumer.source_producer.is_none()
                        && consumer.needs.iter().any(|need| need == id)
                })
                .and_then(|(_, consumer)| consumer.condition.as_deref())
                .and_then(consumer_guard)
                .ok_or(format!("{id} consumer guard"))?;
            producer_keys.push(assert_producer_job(job, id, guard)?);
        } else {
            consumer_keys.push(assert_consumer_job(job, id, &producer_ids)?);
        }
    }
    assert_eq!(consumer_keys.len(), 2, "one consumer per root");
    assert_eq!(producer_ids.len(), 2, "one producer per root");
    consumer_keys.sort();
    producer_keys.sort();
    assert_eq!(
        consumer_keys, producer_keys,
        "consumer restores producer keys"
    );
    assert_ne!(
        producer_keys[0], producer_keys[1],
        "per-root keys stay distinct"
    );
    Ok(())
}

fn assert_producer_job(
    job: &Job,
    id: &str,
    guard: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    assert_eq!(job.display_name, "Public OpenTofu providers", "{id}");
    assert_eq!(
        job.needs,
        vec!["plan".to_owned()],
        "{id} has no consumer obligations"
    );
    let condition = job.condition.as_deref().ok_or(format!("{id} condition"))?;
    assert_eq!(
        condition,
        format!("{CACHE_SAVE_CONDITION} && ({guard})"),
        "{id} preserves the selected consumer guard"
    );
    let metadata = job.source_producer.as_ref().ok_or("producer metadata")?;
    assert_eq!(metadata.role, SourceProducerRole::Tofu, "{id} role");
    assert!(!job.steps.iter().any(|step| {
        step.name == "Init for validate"
            || step.name == "Validate"
            || matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))
    }), "{id} stays source-only");
    let export = job
        .steps
        .iter()
        .position(|step| step.name == "Export verified OpenTofu providers")
        .ok_or(format!("{id} exports providers"))?;
    let save = job
        .steps
        .iter()
        .position(|step| step.name == "Save Tofu providers")
        .ok_or(format!("{id} saves providers"))?;
    assert!(export < save, "{id} verifies before save");
    assert!(
        save_condition(job, save),
        "{id} save is push and verification gated"
    );
    let StepKind::Action { with, .. } = &job.steps[save].kind else {
        return Err(format!("{id} save must be an action step").into());
    };
    let key = with.get("key").ok_or("producer save key")?.clone();
    assert_literal_provider_key(&key, id);
    assert_eq!(
        metadata.source_identity, key,
        "{id} metadata binds save key"
    );
    Ok(key)
}

fn consumer_guard(condition: &str) -> Option<&str> {
    condition
        .strip_prefix("always() && needs.plan.result == 'success' && (")
        .and_then(|guard| guard.strip_suffix(')'))
}

fn save_condition(job: &Job, save: usize) -> bool {
    job.steps[save]
        .condition
        .as_deref()
        .is_some_and(|condition| {
            condition.starts_with(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION)
                && condition.contains("outputs.verified == 'true'")
        })
}

fn assert_consumer_job(
    job: &Job,
    id: &str,
    producer_ids: &[&String],
) -> Result<String, Box<dyn std::error::Error>> {
    let restore = job
        .steps
        .iter()
        .find(|step| step.name == "Restore Tofu providers")
        .ok_or(format!("{id} restores providers"))?;
    let StepKind::Action { with, .. } = &restore.kind else {
        return Err(format!("{id} restore must be an action step").into());
    };
    let key = with.get("key").ok_or("restore key")?.clone();
    assert_literal_provider_key(&key, id);
    assert!(
        job.steps
            .iter()
            .all(|step| step.name != "Save Tofu providers"),
        "{id} consumer never saves"
    );
    assert_eq!(
        job.needs
            .iter()
            .filter(|need| producer_ids.iter().any(|producer| *producer == *need))
            .count(),
        1,
        "{id} waits on one source producer"
    );
    let condition = job.condition.as_deref().ok_or(format!("{id} condition"))?;
    assert!(
        condition.contains("always()"),
        "{id} runs after skipped producer"
    );
    assert!(
        condition.contains("needs.plan.result == 'success'"),
        "{id} still requires Plan success"
    );
    Ok(key)
}

fn assert_literal_provider_key(key: &str, id: &str) {
    assert!(
        key.starts_with("velnor-v2-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-"),
        "{id} key shape: {key}"
    );
    assert!(
        !key.contains("hashFiles(") && !key.contains("${{"),
        "{id} key is literal"
    );
    assert!(
        key.rsplit('-').next().is_some_and(
            |digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        ),
        "{id} key carries source digest: {key}"
    );
}
