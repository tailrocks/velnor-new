//! Crate-job source preparation: fetch precedes obligations on cold runners.
//!
//! Crate payloads run `--locked --offline`, so a cold runner with an empty
//! registry fails every obligation unless the job fetches sources first (the same
//! Gate-1 mechanism the plan job uses).

use std::{collections::BTreeMap, fs};

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::{TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_rust::TaskKind;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
use velnor_actions_workflow_renderer::tree::RenderedTree;

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

/// Parse and validate the task manifest emitted with a rendered tree.
fn task_manifest(
    tree: &RenderedTree,
) -> Result<TaskExecutionManifestV1, Box<dyn std::error::Error>> {
    let marked = tree
        .get(TASK_EXECUTION_MANIFEST_PATH)
        .ok_or_else(|| std::io::Error::other("missing task execution manifest"))?;
    let (_, json) = marked
        .split_once('\n')
        .ok_or_else(|| std::io::Error::other("missing task execution marker"))?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(json)?;
    manifest.validate()?;
    Ok(manifest)
}

/// Verify that the fetch shell step uses the Clippy manifest's environment and digest.
fn assert_fetch_task_binding(
    tree: &RenderedTree,
    task_id: &str,
    fetch_env: &BTreeMap<String, String>,
    run_env: &BTreeMap<String, String>,
) -> TestResult {
    let manifest = task_manifest(tree)?;
    let record = manifest
        .tasks
        .get(task_id)
        .ok_or("missing Clippy task manifest record")?;
    assert_eq!(record.env, *run_env, "manifest copies Clippy env");
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert_eq!(
            fetch_env.get(key),
            run_env.get(key),
            "fetch must share the obligation toolchain home {key}"
        );
        assert!(
            fetch_env.get(key).is_some_and(|value| !value.is_empty()),
            "fetch toolchain home {key} must be set"
        );
    }
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    assert!(
        yaml.contains(&record.execution_digest),
        "rendered Clippy call is bound to its manifest record"
    );
    Ok(())
}

#[test]
fn crate_job_fetches_lockful_sources_before_obligations() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    let prep = prepare(root)?;
    for task in prep
        .discovery
        .proposals
        .iter()
        .filter(|task| !task.no_targets && task.task_kind != TaskKind::Fmt.as_str())
    {
        let argv = args_of(&task.payload);
        assert!(
            argv.contains(&"--locked".to_owned()) && argv.contains(&"--offline".to_owned()),
            "payload stays locked/offline; fetch fixes cold: {argv:?}"
        );
    }
    let tree = render_staged_tree(&prep)?;
    let job = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or_else(|| std::io::Error::other("missing crate job"))?;
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|seen| *seen == name);
    // Cargo-only fixture uses the same shared sources restore as MBX.
    let (Some(prepare_at), Some(cache_at), Some(fetch_at), Some(run_at)) = (
        at("Prepare pinned tools"),
        at("Restore Cargo sources"),
        at("Fetch Cargo sources"),
        at("Clippy"),
    ) else {
        return Err(format!("crate steps miss fetch ordering: {names:?}").into());
    };
    assert!(
        prepare_at < cache_at && cache_at < fetch_at && fetch_at < run_at,
        "cache<fetch<obligations: {names:?}"
    );
    let StepKind::Shell { run, env } = &job.steps[fetch_at].kind else {
        return Err("fetch step must be a shell step".into());
    };
    assert_eq!(&run[..2], ["sh", "-c"]);
    for need in [
        "metadata --locked --offline",
        "cargo fetch --locked",
        "mkdir -p \"$RUNNER_TEMP/velnor/cargo-clean\"",
        "cd \"$RUNNER_TEMP/velnor/cargo-clean\"",
        "--manifest-path \"$GITHUB_WORKSPACE/Cargo.toml\"",
    ] {
        assert!(run[2].contains(need), "fetch script misses {need}");
    }
    let StepKind::TaskExecution {
        task_id,
        env: run_env,
        ..
    } = &job.steps[run_at].kind
    else {
        return Err("Clippy must be a declared task".into());
    };
    assert_fetch_task_binding(&tree, task_id, env, run_env)?;
    Ok(())
}

/// Rendered crate-job YAML window for a lockful fixture repo.
fn lockful_crate_window() -> Result<(String, TaskExecutionManifestV1), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("Cargo.lock"), demo_lock("demo"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let manifest = task_manifest(&tree)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    let task_at = yaml
        .find("  rust-demo:")
        .ok_or_else(|| std::io::Error::other("missing crate job"))?;
    Ok((yaml[task_at..].to_owned(), manifest))
}

#[test]
fn rendered_crate_fetch_precedes_obligations_with_toolchain_triple() -> TestResult {
    let (window, manifest) = lockful_crate_window()?;
    let fetch_pos = window
        .find("Fetch Cargo sources")
        .ok_or_else(|| std::io::Error::other("rendered fetch missing"))?;
    let run_pos = window
        .find("- name: Clippy")
        .ok_or_else(|| std::io::Error::other("rendered obligation missing"))?;
    assert!(fetch_pos < run_pos, "rendered fetch must precede Clippy");
    assert!(
        window.contains("cargo fetch --locked"),
        "rendered fetch argv:\n{window}"
    );
    let fetch_window = &window[fetch_pos..run_pos];
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:"] {
        assert!(
            fetch_window.contains(key),
            "rendered fetch must carry {key}:\n{window}"
        );
    }
    let clippy = manifest
        .tasks
        .values()
        .find(|record| record.argv.iter().any(|arg| arg == "clippy"))
        .ok_or("missing Clippy manifest record")?;
    assert!(
        clippy
            .env
            .get("RUSTUP_TOOLCHAIN")
            .is_some_and(|value| value == "1.99.0"),
        "Clippy record must carry the catalog Rust toolchain: {:?}",
        clippy.env
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME"] {
        assert!(
            clippy.env.get(key).is_some_and(|value| !value.is_empty()),
            "Clippy record must carry {key}: {:?}",
            clippy.env
        );
    }
    assert!(
        window.contains(&clippy.execution_digest),
        "rendered Clippy call binds its manifest record:\n{window}"
    );
    Ok(())
}

#[test]
fn crate_job_omits_fetch_without_lockfile() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let job = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or_else(|| std::io::Error::other("missing crate job"))?;
    assert!(
        job.steps
            .iter()
            .all(|step| !step.name.starts_with("Fetch Cargo sources")),
        "lockless workspaces have nothing to fetch"
    );
    Ok(())
}

#[test]
fn mbx_objects_precede_fetch_on_mbx_crates() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    with_mbx(root)?;
    let prep = prepare(root)?;
    let job = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or_else(|| std::io::Error::other("missing crate job"))?;
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|seen| *seen == name);
    // P08: restore shared sources, configure MBX, then probe-and-fetch.
    let (Some(restore_at), Some(objects_at), Some(fetch_at), Some(run_at)) = (
        at("Restore Cargo sources"),
        at("Restore MBX objects"),
        at("Fetch Cargo sources"),
        at("Clippy"),
    ) else {
        return Err(format!("mbx crate misses source/object order: {names:?}").into());
    };
    assert!(
        restore_at < objects_at && objects_at < fetch_at && fetch_at < run_at,
        "restore<objects<fetch<obligations: {names:?}"
    );
    assert!(
        names
            .iter()
            .filter(|name| **name == "Restore Cargo sources")
            .count()
            == 1,
        "MBX crate has one exact sources restore: {names:?}"
    );
    Ok(())
}
