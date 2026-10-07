//! T27 tofu rendering cases: strict config surface, backend-less
//! validation, and provider-free versus provider-backed runs.
//!
//! `[stacks.tofu]` admits roots only — raw YAML keys and custom
//! shells fail closed at load, and rendered legs carry the fixed
//! payload with the default shell. Validation never touches state:
//! even a nondefault `backend` block plans and validates through
//! `init -backend=false`. Provider-free roots run silent without a
//! lockfile; provider-backed roots fail planning without a committed
//! lock (readonly init would fail them in CI).
use std::fs;
use std::path::Path;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract_workflow::{FinalStatus, Plan};
use velnor_actions_orchestrator::{finalized_jobs, plan_internal, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::cases::orch_core::{merge, merge_request, success_jobs};
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

/// Pure-tofu config over `roots` with one extra `[stacks.tofu]` line.
fn config_with_extra(roots: &str, extra: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [{roots}]\n{extra}\n"
    )
}

/// Staged `ci.yml` text `generate` would write for one repo root.
fn staged_yaml(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(&prepare(root)?)?;
    tree.get(WORKFLOW_PATH)
        .map(str::to_owned)
        .ok_or_else(|| "missing workflow in staged tree".into())
}

/// Plan the pull request ending at `head` over a two-commit tofu repo.
fn plan_tofu_pr(root: &Path) -> Result<Plan, Box<dyn std::error::Error>> {
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
    Ok(plan)
}

/// Raw YAML keys under `[stacks.tofu]` fail closed at load.
#[test]
fn tofu_config_rejects_raw_yaml_keys() -> TestResult {
    for extra in [
        "shell = \"bash -c 'tofu apply'\"",
        "run = \"tofu apply -auto-approve\"",
        "steps = [\"apply\"]",
        "yaml = \"jobs: {}\"",
    ] {
        let config = config_with_extra("\"stacks/a\"", extra);
        let dir = make_pure_tofu_repo(&config, &[("stacks/a/main.tf", "variable \"x\" {}\n")])?;
        assert!(
            prepare(dir.path()).is_err(),
            "extra key must fail closed: {extra}"
        );
    }
    Ok(())
}

/// Rendered tofu legs carry the fixed payload under the default shell.
#[test]
fn tofu_rendered_steps_carry_no_custom_shell() -> TestResult {
    let config = config_with_extra("\"stacks/a\", \"stacks/b\"", "");
    let dir = make_pure_tofu_repo(
        &config,
        &[
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
            ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ],
    )?;
    let yaml = staged_yaml(dir.path())?;
    assert!(
        !yaml
            .lines()
            .any(|line| line.trim_start().starts_with("shell:")),
        "no custom shell anywhere:\n{yaml}"
    );
    let mut tofu_runs = 0;
    for line in yaml.lines() {
        if line.contains("-chdir stacks/") {
            assert!(
                line.starts_with("        run: "),
                "single-line scalar: {line}"
            );
            assert!(line.contains("tofu"), "fixed tofu payload: {line}");
            tofu_runs += 1;
        }
    }
    assert!(tofu_runs >= 2, "both roots render legs:\n{yaml}");
    Ok(())
}

/// A nondefault backend never sees backend access during validation.
#[test]
fn tofu_nondefault_backend_validates_without_backend_access() -> TestResult {
    let config = config_with_extra("\"stacks/a\"", "");
    let backend = "terraform {\n  backend \"s3\" {\n    bucket = \"state\"\n    key = \"a.tfstate\"\n  }\n}\nvariable \"a\" {}\n";
    let dir = make_pure_tofu_repo(&config, &[("stacks/a/main.tf", backend)])?;
    let root = dir.path();
    prepare(root)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(
        root.join("stacks/a/main.tf"),
        format!("{backend}variable \"b\" {{}}\n"),
    )?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let plan = plan_tofu_pr(root)?;
    assert_eq!(plan.obligations.len(), 3, "one root plans the triple");
    let mut init_seen = false;
    for entry in &plan.matrix.include {
        assert!(
            !entry.run.contains("-backend=true") && !entry.run.contains("backend-config"),
            "no backend access: {}",
            entry.run
        );
        if entry.task_id.contains("/init/") {
            assert!(
                entry.run.contains("init -backend=false -input=false"),
                "backend-less init: {}",
                entry.run
            );
            init_seen = true;
        }
    }
    assert!(init_seen, "init leg planned");
    let jobs = finalized_jobs(&prepare(root)?)?;
    let job = jobs.get("tofu-stacks-a").ok_or("tofu job")?;
    let init = job
        .steps
        .iter()
        .find(|step| step.name == "Init for validate")
        .ok_or("init step")?;
    let body = serde_json::to_string(&init.kind)?;
    assert!(
        body.contains("-backend=false"),
        "rendered init stays backend-less: {body}"
    );
    Ok(())
}

/// A provider-free root runs silent with no lockfile and merges green.
#[test]
fn tofu_provider_free_root_runs_without_lock_claims() -> TestResult {
    let config = config_with_extra("\"stacks/a\"", "");
    let dir = make_pure_tofu_repo(&config, &[("stacks/a/main.tf", "variable \"a\" {}\n")])?;
    let root = dir.path();
    let prep = prepare(root)?;
    assert!(
        !prep
            .discovery
            .recommendations
            .iter()
            .any(|line| line.contains("tofu_lockfile")),
        "provider-free stays silent: {:?}",
        prep.discovery.recommendations
    );
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"b\" {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let plan = plan_tofu_pr(root)?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    Ok(())
}

/// A provider-backed root without a committed lock fails planning:
/// readonly init would fail it in CI, so no plan and no merge follow.
#[test]
fn tofu_provider_backed_root_without_lock_fails_planning() -> TestResult {
    let config = config_with_extra("\"stacks/a\"", "");
    let backed = "resource \"null_resource\" \"x\" {}\n";
    let dir = make_pure_tofu_repo(&config, &[("stacks/a/main.tf", backed)])?;
    let root = dir.path();
    let err = prepare(root).expect_err("missing lock must fail planning");
    assert!(
        err.to_string()
            .contains("missing_committed_lock:stacks/a/.terraform.lock.hcl"),
        "names the lock: {err}"
    );
    Ok(())
}
