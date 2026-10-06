use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::{ObligationDecision, Plan};
use velnor_actions_mise::cache_sources as mise_sources;
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, plan_internal, prepare,
};
use velnor_actions_tofu::TofuLockSnapshot;
use velnor_actions_workflow_renderer::steps as renderer_steps;

use super::impl_common::{TestResult, git, git_line, make_repo};

const PUBLIC_PROVIDER_LOCK: &str = r#"provider "registry.opentofu.org/hashicorp/null" {
  version = "3.2.1"
  hashes = ["h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]
}
"#;

fn tofu_config(root: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"{root}\"]\n"
    )
}

fn stale_lock() -> &'static str {
    "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\nhashes = [\"h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"]\n}\n"
}

fn corrupt_lock() -> &'static str {
    "provider \"x\" {\n  version===\n"
}

fn unpinned_lock() -> &'static str {
    "provider \"registry.opentofu.org/hashicorp/null\" {\nversion = \"3.2.1\"\n}\n"
}

fn has_line(prep: &velnor_actions_orchestrator::GenerationPreparation, needle: &str) -> bool {
    prep.discovery
        .recommendations
        .iter()
        .any(|line| line.contains(needle))
}

fn has_step(job: &velnor_actions_contract::Job, name: &str) -> bool {
    job.steps.iter().any(|step| step.name == name)
}

fn consumer_guard(condition: &str) -> Option<&str> {
    condition
        .strip_prefix("always() && needs.plan.result == 'success' && (")
        .and_then(|guard| guard.strip_suffix(')'))
}

fn has_provider_restore(job: &velnor_actions_contract::Job) -> bool {
    has_step(job, "Restore Tofu providers") && !has_step(job, "Save Tofu providers")
}

fn assert_no_provider_cache(job: &velnor_actions_contract::Job, id: &str) {
    assert!(
        !has_step(job, "Restore Tofu providers")
            && !has_step(job, "Save Tofu providers")
            && has_step(job, "Init for validate")
            && has_step(job, "Validate"),
        "{id} cache/validation"
    );
}

#[test]
fn never_archive_mirrors_stay_equal_across_crates() {
    assert_eq!(
        renderer_steps::NEVER_ARCHIVE_MARKERS,
        mise_sources::NEVER_ARCHIVE_MARKERS,
        "rendered steps and local checks share one never-archive contract"
    );
    for path in [
        "/cache/registry/cache/state.tfstate",
        "/cache/registry/cache/state.tfstate.backup",
        "/cache/registry/cache/plan.tfplan",
        "/cache/registry/cache/credentials.toml",
        "/cache/registry/cache/serde-1.0.228.crate",
        "/cache/.crates.toml",
        "/cache/bin/cargo-nextest",
    ] {
        assert_eq!(
            renderer_steps::is_never_archive_path(path),
            mise_sources::is_never_archive_path(path),
            "never-archive verdict parity for {path}"
        );
    }
}

#[test]
fn finalized_tofu_jobs_order_restore_before_work_before_save() -> TestResult {
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(
        root.join("stacks/a/.terraform.lock.hcl"),
        PUBLIC_PROVIDER_LOCK,
    )?;
    let jobs = finalized_jobs(&prepare(root)?)?;
    let (producer_id, producer) = jobs
        .iter()
        .find(|(_, job)| job.source_producer.is_some())
        .ok_or("source producer")?;
    let save = producer
        .steps
        .iter()
        .find(|step| step.name == "Save Tofu providers")
        .ok_or("producer saves")?;
    assert!(
        save.condition
            .as_deref()
            .is_some_and(|condition| condition.starts_with(CACHE_SAVE_CONDITION)),
        "producer save is push gated"
    );
    let (id, job) = jobs
        .iter()
        .find(|(id, job)| id.starts_with("tofu-") && job.source_producer.is_none())
        .ok_or("tofu consumer")?;
    let at = |want: &str| job.steps.iter().position(|step| step.name == want);
    let restore = at("Restore Tofu providers").ok_or(format!("{id} restores"))?;
    let init = at("Init for validate").ok_or(format!("{id} runs init"))?;
    assert!(restore < init, "{id} orders restore before init");
    assert!(
        job.steps
            .iter()
            .all(|step| step.name != "Save Tofu providers"),
        "{id} consumer never saves"
    );
    assert!(
        job.needs.iter().any(|need| need == producer_id),
        "{id} waits on producer"
    );
    let condition = job.condition.as_deref().ok_or(format!("{id} condition"))?;
    assert!(condition.contains("always()") && condition.contains("needs.plan.result == 'success'"));
    renderer_steps::check_cache_step_order(&job.steps).map_err(|err| format!("{id}: {err}"))?;
    Ok(())
}

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

#[test]
fn missing_lock_plans_validation_without_seeded_provider_cache() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n";
    let dir = make_pure_tofu_repo(config, &[("stacks/a/main.tf", "variable \"x\" {}\n")])?;
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
    assert!(!plan.obligations.is_empty(), "cold tofu plans obligations");
    for obligation in &plan.obligations {
        assert_eq!(
            obligation.decision,
            ObligationDecision::Execute,
            "{} executes cold",
            obligation.task_id
        );
    }
    let prep = prepare(root)?;
    let roots = ["stacks/a".to_owned()];
    let snap = TofuLockSnapshot::capture(&prep.root, &roots);
    generate(&prep, &GenerateOptions { output_dir: None })?;
    assert!(snap.verify(&prep.root).is_ok(), "generate writes nothing");
    let jobs = finalized_jobs(&prep)?;
    for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
        assert_no_provider_cache(job, id);
    }
    Ok(())
}

#[test]
fn corrupt_lock_diagnoses_and_skips_provider_transport() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), corrupt_lock())?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_lockfile_corrupt"),
        "{:?}",
        prep.discovery.recommendations
    );
    let jobs = finalized_jobs(&prep)?;
    assert!(
        jobs.iter()
            .filter(|(id, _)| id.starts_with("tofu-"))
            .all(|(_, job)| {
                assert_no_provider_cache(job, "corrupt");
                true
            }),
        "a corrupt lock keeps validation without provider transport"
    );
    Ok(())
}

#[test]
fn missing_lock_fails_planning_before_provider_transport_renders() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "resource \"x\" \"y\" {}\n")?;
    let err = prepare(root).expect_err("missing lock must fail planning");
    assert!(
        err.to_string()
            .contains("missing_committed_lock:stacks/a/.terraform.lock.hcl"),
        "names the lock: {err}"
    );
    Ok(())
}

#[test]
fn custom_lock_diagnoses_and_skips_provider_transport() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), stale_lock())?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_lockfile_stale"),
        "{:?}",
        prep.discovery.recommendations
    );
    let jobs = finalized_jobs(&prep)?;
    assert!(
        jobs.iter()
            .filter(|(id, _)| id.starts_with("tofu-"))
            .all(|(_, job)| {
                assert_no_provider_cache(job, "stale");
                true
            }),
        "a custom-source lock keeps validation without provider transport"
    );
    Ok(())
}

#[test]
fn unpinned_lock_skips_provider_transport_but_keeps_validation() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), unpinned_lock())?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_lockfile_unpinned_hashes"),
        "{:#?}",
        prep.discovery.recommendations
    );
    let jobs = finalized_jobs(&prep)?;
    for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
        assert_no_provider_cache(job, id);
    }
    Ok(())
}

#[test]
fn private_and_mixed_locks_skip_provider_transport_but_keep_validation() -> TestResult {
    let private = PUBLIC_PROVIDER_LOCK.replace(
        "registry.opentofu.org/hashicorp/null",
        "private.example.com/acme/internal",
    );
    let mixed = format!("{PUBLIC_PROVIDER_LOCK}\n{private}");
    for (label, lock) in [("private", private.as_str()), ("mixed", mixed.as_str())] {
        let repo = make_repo(&tofu_config("stacks/a"))?;
        let root = repo.path();
        fs::create_dir_all(root.join("stacks/a"))?;
        fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
        fs::write(root.join("stacks/a/.terraform.lock.hcl"), lock)?;
        let jobs = finalized_jobs(&prepare(root)?)?;
        for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
            assert_no_provider_cache(job, &format!("{label}:{id}"));
        }
    }
    Ok(())
}

#[test]
fn version_excluding_toolchain_diagnoses_without_disabling_transport() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(
        root.join("stacks/a/main.tf"),
        "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
    )?;
    fs::write(
        root.join("stacks/a/.terraform.lock.hcl"),
        PUBLIC_PROVIDER_LOCK,
    )?;
    fs::write(root.join("mise.toml"), "[tools]\nopentofu = \"1.13.1\"\n")?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_required_version_excludes_toolchain"),
        "{:?}",
        prep.discovery.recommendations
    );
    let jobs = finalized_jobs(&prep)?;
    assert!(
        jobs.iter()
            .filter(|(id, job)| id.starts_with("tofu-") && job.source_producer.is_none())
            .all(|(_, job)| has_provider_restore(job)),
        "a version skew never breaks provider transport"
    );
    Ok(())
}

#[test]
fn fork_event_plans_pr_trust_and_generates_restore_only_roundtrip() -> TestResult {
    use super::impl_orch_core::plan_value;
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n";
    let dir = make_pure_tofu_repo(
        config,
        &[
            ("stacks/a/main.tf", "variable \"x\" {}\n"),
            ("stacks/a/.terraform.lock.hcl", PUBLIC_PROVIDER_LOCK),
        ],
    )?;
    let root = dir.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    git(&["commit", "--allow-empty", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let value = plan_value(root, "fork", Some(&base), &head, None)?;
    assert_eq!(value["plan"]["trust"], "pr", "fork plans distrust");
    let jobs = finalized_jobs(&prepare(root)?)?;
    let (producer_id, producer) = jobs
        .iter()
        .find(|(_, job)| job.source_producer.is_some())
        .ok_or("source producer")?;
    let (id, job) = jobs
        .iter()
        .find(|(id, job)| id.starts_with("tofu-") && job.source_producer.is_none())
        .ok_or("tofu consumer")?;
    assert!(
        has_step(job, "Restore Tofu providers"),
        "{id} restores on fork"
    );
    assert!(
        !has_step(job, "Save Tofu providers"),
        "{id} consumer never saves"
    );
    assert!(
        job.needs.iter().any(|need| need == producer_id),
        "{id} waits on producer"
    );
    let condition = job.condition.as_deref().ok_or(format!("{id} condition"))?;
    assert!(condition.contains("always()"));
    assert!(condition.contains("needs.plan.result == 'success'"));
    let guard = consumer_guard(condition).ok_or("consumer guard")?;
    let producer_condition = producer.condition.as_deref().ok_or("producer condition")?;
    assert!(
        producer_condition.starts_with(CACHE_SAVE_CONDITION) && producer_condition.contains(guard),
        "producer preserves push gate and selected guard"
    );
    Ok(())
}
