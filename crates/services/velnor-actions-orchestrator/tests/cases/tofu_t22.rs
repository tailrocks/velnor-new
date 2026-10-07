//! T22 provider-cache cold recovery: never-archive mirror pins,
//! restore-before-work ordering, and lock/version negatives that
//! diagnose without breaking provider transport.
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract_workflow::{ObligationDecision, Plan, Step, StepKind, StepRole};
use velnor_actions_mise::cache_sources as mise_sources;
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, plan_internal, prepare,
};
use velnor_actions_tofu_core::TofuLockSnapshot;
use velnor_actions_workflow_cache::cache_steps as renderer_steps;

use crate::support::{TestResult, git, git_line, install_fixture_release_manifest, make_repo};

/// Fixture config with one tofu root.
fn tofu_config(root: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"{root}\"]\n"
    )
}

/// Stale lock bytes: pins a provider no config requires.
fn stale_lock() -> &'static str {
    "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n}\n"
}

/// Corrupt lock bytes: unparseable HCL.
fn corrupt_lock() -> &'static str {
    "provider \"x\" {\n  version===\n"
}

/// True when any recommendation line contains `needle`.
fn has_line(prep: &velnor_actions_orchestrator::GenerationPreparation, needle: &str) -> bool {
    prep.discovery
        .recommendations
        .iter()
        .any(|line| line.contains(needle))
}

/// True when a tofu job restores and saves its provider cache.
fn has_provider_roundtrip(job: &velnor_actions_contract_workflow::Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.name == "Restore Tofu providers")
        && job
            .steps
            .iter()
            .any(|step| step.name == "Save Tofu providers")
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
    use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    let jobs = finalized_jobs(&prepare(root)?)?;
    let mut seen = 0;
    for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
        seen += 1;
        let at = |want: &str| job.steps.iter().position(|step| step.name == want);
        let restore = at("Restore Tofu providers").ok_or(format!("{id} restores"))?;
        let init = at("Init for validate").ok_or(format!("{id} runs init"))?;
        let save = at("Save Tofu providers").ok_or(format!("{id} saves"))?;
        assert!(
            restore < init && init < save,
            "{id} orders restore < init < save"
        );
        assert_eq!(
            job.steps[save].condition.as_deref(),
            Some(CACHE_SAVE_CONDITION),
            "{id} saves under the push-only gate"
        );
        renderer_steps::check_cache_step_order(&job.steps).map_err(|err| format!("{id}: {err}"))?;
    }
    assert_eq!(seen, 1, "one tofu job for one root");
    Ok(())
}

#[test]
fn provider_admission_discards_prefix_and_missing_matches() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    fs::create_dir_all(repo.path().join("stacks/a"))?;
    fs::write(repo.path().join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let restore = jobs
        .values()
        .flat_map(|job| &job.steps)
        .find(|step| step.role == Some(StepRole::TofuProvidersRestore))
        .ok_or("finalized tofu job misses provider restore/admission")?;
    let StepKind::Action { uses, with, .. } = &restore.kind else {
        return Err("provider restore/admission is not a local action call".into());
    };
    assert_eq!(
        uses,
        velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
    );
    let expected = with
        .get("cache-key")
        .ok_or("provider composite misses key")?;
    let cache_path = with
        .get("cache-path")
        .ok_or("provider composite misses owned path")?;
    let temp = tempfile::tempdir()?;
    let runner_temp = temp.path().canonicalize()?;
    let resolved_path =
        cache_path.replace("${{ runner.temp }}", &runner_temp.display().to_string());
    let leaf = Path::new(&resolved_path);
    fs::create_dir_all(leaf)?;
    fs::write(leaf.join("verified-provider"), b"cached")?;

    let exact = run_admission(restore, &runner_temp, "true", expected, false)?;
    assert!(
        exact.status.success(),
        "{}",
        String::from_utf8_lossy(&exact.stderr)
    );
    assert!(
        leaf.join("verified-provider").exists(),
        "exact hit is retained"
    );

    for (hit, matched) in [
        ("true", format!("{expected}-older-prefix")),
        ("false", String::new()),
    ] {
        fs::write(leaf.join("verified-provider"), b"must be removed")?;
        let rejected = run_admission(restore, &runner_temp, hit, &matched, true)?;
        assert!(
            rejected.status.success(),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }
    Ok(())
}

fn run_admission(
    step: &Step,
    runner_temp: &Path,
    hit: &str,
    matched: &str,
    verify_cleared: bool,
) -> std::io::Result<Output> {
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(std::io::Error::other(
            "admission step is not an action step",
        ));
    };
    let expected = with
        .get("cache-key")
        .ok_or_else(|| std::io::Error::other("provider composite misses key"))?;
    let cache_path = with
        .get("cache-path")
        .ok_or_else(|| std::io::Error::other("provider composite misses path"))?;
    let resolved_path =
        cache_path.replace("${{ runner.temp }}", &runner_temp.display().to_string());
    let script = if verify_cleared {
        format!(
            "{}; test ! -e \"$TOFU_PROVIDER_CACHE_PATH/verified-provider\"",
            velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_ADMISSION_SCRIPT
        )
    } else {
        velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_ADMISSION_SCRIPT.to_owned()
    };
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(script)
        .env("RUNNER_TEMP", runner_temp);
    command
        .env("TOFU_CACHE_HIT", hit)
        .env("TOFU_MATCHED_KEY", matched)
        .env("TOFU_EXPECTED_KEY", expected)
        .env("TOFU_PROVIDER_CACHE_PATH", resolved_path);
    command.output()
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

/// A cold tree (no cache seeded anywhere) still plans every tofu
/// obligation as execute and generates provider steps read-only.
#[test]
fn cold_tree_plans_executes_and_generates_without_seeded_cache() -> TestResult {
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
    assert!(
        jobs.iter()
            .filter(|(id, _)| id.starts_with("tofu-"))
            .all(|(_, job)| has_provider_roundtrip(job)),
        "cold tofu jobs still render the provider roundtrip"
    );
    Ok(())
}

#[test]
fn corrupt_lock_diagnoses_while_provider_transport_renders() -> TestResult {
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
            .all(|(_, job)| has_provider_roundtrip(job)),
        "a corrupt lock never breaks provider transport"
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
fn stale_lock_diagnoses_while_provider_transport_renders() -> TestResult {
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
            .all(|(_, job)| has_provider_roundtrip(job)),
        "a stale lock never breaks provider transport"
    );
    Ok(())
}

#[test]
fn version_excluding_toolchain_diagnoses_while_provider_transport_renders() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(
        root.join("stacks/a/main.tf"),
        "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
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
            .filter(|(id, _)| id.starts_with("tofu-"))
            .all(|(_, job)| has_provider_roundtrip(job)),
        "a version skew never breaks provider transport"
    );
    Ok(())
}

/// Fork composition: a fork event plans at PR trust and generates
/// restore-everywhere plus push-gated saves, so fork runs are
/// restore-only at runtime.
#[test]
fn fork_event_plans_pr_trust_and_generates_restore_only_roundtrip() -> TestResult {
    use crate::cases::orch_core::plan_value;
    use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n";
    let dir = make_pure_tofu_repo(config, &[("stacks/a/main.tf", "variable \"x\" {}\n")])?;
    let root = dir.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    git(&["commit", "--allow-empty", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let value = plan_value(root, "fork", Some(&base), &head, None)?;
    assert_eq!(value["plan"]["trust"], "pr", "fork plans distrust");
    let jobs = finalized_jobs(&prepare(root)?)?;
    let mut seen = 0;
    for (id, job) in jobs.iter().filter(|(id, _)| id.starts_with("tofu-")) {
        seen += 1;
        let at = |want: &str| job.steps.iter().position(|step| step.name == want);
        at("Restore Tofu providers").ok_or(format!("{id} restores"))?;
        let save = at("Save Tofu providers").ok_or(format!("{id} saves"))?;
        assert_eq!(
            job.steps[save].condition.as_deref(),
            Some(CACHE_SAVE_CONDITION),
            "{id} saves push-gated only: fork runs restore without saving"
        );
    }
    assert_eq!(seen, 1, "one tofu job for one root");
    Ok(())
}
