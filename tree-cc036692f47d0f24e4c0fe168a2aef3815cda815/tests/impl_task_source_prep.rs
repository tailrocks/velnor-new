//! Task-job source preparation: fetch precedes `Run task` on cold runners.
//!
//! Task payloads run `--locked --offline`, so a cold runner with an empty
//! registry fails every leg unless the job fetches sources first (the same
//! Gate-1 mechanism the plan job uses).

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_rust::TaskKind;
use velnor_actions_rust::tasks::cargo_payload_argv;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, make_repo};

/// Hand-written lock for a no-deps fixture package: hermetic, no network.
fn demo_lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// Select MBX for the fixture crate via `rustc-wrapper` evidence.
fn with_mbx(root: &std::path::Path) -> TestResult {
    let cargo_dir = root.join(".cargo");
    fs::create_dir_all(&cargo_dir)?;
    fs::write(
        cargo_dir.join("config.toml"),
        "[build]\nrustc-wrapper = \"mbx\"\n",
    )?;
    Ok(())
}

/// Lossy argv strings for flag assertions.
fn args_of(argv: &[std::ffi::OsString]) -> Vec<String> {
    argv.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn task_job_fetches_lockful_sources_before_run_task() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    let prep = prepare(root)?;
    for group in prep
        .discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets && group.kind != TaskKind::Fmt)
    {
        let argv = args_of(&cargo_payload_argv(group));
        assert!(
            argv.contains(&"--locked".to_owned()) && argv.contains(&"--offline".to_owned()),
            "payload stays locked/offline; fetch fixes cold: {argv:?}"
        );
    }
    let task = prep
        .workflow
        .ir
        .jobs
        .get("velnor-task")
        .ok_or_else(|| std::io::Error::other("missing task job"))?;
    let names: Vec<&str> = task.steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|seen| *seen == name);
    let (Some(prepare_at), Some(fetch_at), Some(run_at)) = (
        at("Prepare pinned tools"),
        at("Fetch Cargo sources"),
        at("Run task"),
    ) else {
        return Err(format!("task steps miss fetch ordering: {names:?}").into());
    };
    assert!(
        prepare_at < fetch_at && fetch_at < run_at,
        "fetch must precede Run task: {names:?}"
    );
    let StepKind::Shell { run, env } = &task.steps[fetch_at].kind else {
        return Err("fetch step must be a shell step".into());
    };
    assert!(
        run.windows(2).any(|pair| pair == ["cargo", "fetch"])
            && run.contains(&"--locked".to_owned()),
        "fetch argv must be cargo fetch --locked: {run:?}"
    );
    let StepKind::Shell { env: run_env, .. } = &task.steps[run_at].kind else {
        return Err("Run task must be a shell step".into());
    };
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert_eq!(
            env.get(key),
            run_env.get(key),
            "fetch must share the Run task toolchain home {key}"
        );
        assert!(
            env.get(key).is_some_and(|value| !value.is_empty()),
            "fetch toolchain home {key} must be set"
        );
    }
    Ok(())
}

/// Rendered task-job YAML window for a lockful fixture repo.
fn lockful_task_window() -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("Cargo.lock"), demo_lock("demo"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    let task_at = yaml
        .find("  velnor-task:")
        .ok_or_else(|| std::io::Error::other("missing task job"))?;
    let lint_at = yaml.find("  velnor-workflow-lint:").unwrap_or(yaml.len());
    Ok(yaml[task_at..lint_at].to_owned())
}

#[test]
fn rendered_task_fetch_precedes_run_task_with_toolchain_triple() -> TestResult {
    let window = lockful_task_window()?;
    let fetch_pos = window
        .find("Fetch Cargo sources")
        .ok_or_else(|| std::io::Error::other("rendered fetch missing"))?;
    let run_pos = window
        .find("- name: Run task")
        .ok_or_else(|| std::io::Error::other("rendered run missing"))?;
    assert!(fetch_pos < run_pos, "rendered fetch must precede Run task");
    assert!(
        window.contains("cargo fetch --locked"),
        "rendered fetch argv:\n{window}"
    );
    let fetch_window = &window[fetch_pos..run_pos];
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:", "RUSTUP_TOOLCHAIN:"] {
        assert!(
            fetch_window.contains(key),
            "rendered fetch must carry {key}:\n{window}"
        );
    }
    Ok(())
}

#[test]
fn task_job_omits_fetch_without_lockfile() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let task = prep
        .workflow
        .ir
        .jobs
        .get("velnor-task")
        .ok_or_else(|| std::io::Error::other("missing task job"))?;
    assert!(
        task.steps
            .iter()
            .all(|step| !step.name.starts_with("Fetch Cargo sources")),
        "lockless workspaces have nothing to fetch"
    );
    Ok(())
}

#[test]
fn task_fetch_precedes_mbx_objects_on_mbx_legs() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    with_mbx(root)?;
    let prep = prepare(root)?;
    let task = prep
        .workflow
        .ir
        .jobs
        .get("velnor-task")
        .ok_or_else(|| std::io::Error::other("missing task job"))?;
    let names: Vec<&str> = task.steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|seen| *seen == name);
    let (Some(fetch_at), Some(objects_at), Some(run_at)) = (
        at("Fetch Cargo sources"),
        at("Restore MBX objects"),
        at("Run task"),
    ) else {
        return Err(format!("mbx leg misses source/object order: {names:?}").into());
    };
    assert!(
        fetch_at < objects_at && objects_at < run_at,
        "sources before objects before Run task: {names:?}"
    );
    Ok(())
}
