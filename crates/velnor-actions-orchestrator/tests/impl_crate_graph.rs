//! Crate-job graph regressions: one ordered job per crate.
//!
//! Split from `impl_wire_w1`: generated-YAML shape, per-crate drivers,
//! obligation ordering, toolchain sharing, and cache silence.

use std::fs;

use tempfile::TempDir;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, make_repo};

/// Staged workflow text for one config; temp keeps the dir alive.
fn preview_yaml(config: &str) -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow")?
        .to_owned();
    Ok((repo, yaml))
}

/// Select MBX for the fixture crate via `rustc-wrapper` evidence.
fn with_mbx(repo: &TempDir) -> Result<(), Box<dyn std::error::Error>> {
    let cargo_dir = repo.path().join(".cargo");
    fs::create_dir_all(&cargo_dir)?;
    fs::write(
        cargo_dir.join("config.toml"),
        "[build]\nrustc-wrapper = \"mbx\"\n",
    )?;
    Ok(())
}

/// YAML slice between two markers (to end when `end` is absent).
fn window<'a>(text: &'a str, start: &str, end: &str) -> Result<&'a str, &'static str> {
    let from = text.find(start).ok_or("window start")?;
    let tail = &text[from..];
    match tail.find(end) {
        Some(at) => Ok(&tail[..at]),
        None => Ok(tail),
    }
}

/// Trimmed `env:` line starting with `key` inside one rendered step block.
fn env_line<'a>(block: &'a str, key: &str) -> Result<&'a str, &'static str> {
    for line in block.lines() {
        if line.trim().starts_with(key) {
            return Ok(line.trim());
        }
    }
    Err("env line")
}

/// Locate a named rendered step, failing when the marker is absent.
fn step_at(task: &str, name: &str, missing: &'static str) -> Result<usize, &'static str> {
    task.find(&format!("- name: {name}")).ok_or(missing)
}

/// Assert the Cargo source restore and local MBX setup for a crate job.
fn assert_mbx_setup(task: &str) -> TestResult {
    let source_restore = step_at(task, "Restore Cargo sources", "source restore")?;
    let private_store = step_at(task, "Prepare private MBX store", "private MBX store")?;
    let setup = step_at(task, "Setup MBX", "MBX setup")?;
    let key = step_at(task, "Prepare MBX bundle key", "MBX bundle key")?;
    assert_eq!(
        task.matches("- name: Setup MBX").count(),
        1,
        "one MBX setup step"
    );
    assert!(
        source_restore < private_store && private_store < setup && setup < key,
        "source restore/private store/local MBX setup order:\n{task}"
    );

    let source_cache = &task[source_restore..private_store];
    assert!(
        source_cache.contains("uses: actions/cache/restore@"),
        "explicit Cargo source restore:\n{source_cache}"
    );
    let private = &task[private_store..setup];
    assert!(
        private.contains("mktemp -d")
            && private.contains("velnor-mbx-store.XXXXXXXXXX")
            && private.contains("MBX_CACHE_DIR=%s"),
        "fresh private MBX store:\n{private}"
    );
    let local_setup = &task[setup..key];
    assert!(
        local_setup.contains("uses: jdx/mr-boxington-action@")
            && local_setup.contains("backend: local"),
        "local MBX action setup:\n{local_setup}"
    );
    assert!(
        !task.contains("github-cache-mode"),
        "retired cache mode:\n{task}"
    );
    Ok(())
}

/// Assert MBX bundle restore/import, source fetch, and all crate builds precede export.
fn assert_mbx_bundle_order(task: &str) -> TestResult {
    let key = step_at(task, "Prepare MBX bundle key", "MBX bundle key")?;
    let restore = step_at(task, "Restore MBX single bundle", "bundle restore")?;
    let import = step_at(task, "Import MBX single bundle", "bundle import")?;
    let fetch = step_at(task, "Fetch Cargo sources", "source fetch")?;
    let clippy = step_at(task, "Clippy", "first MBX build")?;
    let tests = step_at(task, "Unit and integration tests", "MBX tests")?;
    let doctests = step_at(task, "Doctests", "MBX doctests")?;
    let documentation = step_at(task, "Documentation", "last MBX build")?;
    let export = step_at(task, "Export MBX single bundle", "bundle export")?;
    let save = step_at(task, "Save MBX single bundle", "bundle save")?;
    assert!(
        key < restore
            && restore < import
            && import < fetch
            && fetch < clippy
            && clippy < tests
            && tests < doctests
            && doctests < documentation
            && documentation < export
            && export < save,
        "bundle restore/import, Cargo fetch/build, export/save order:\n{task}"
    );
    let cargo_fetch = &task[fetch..clippy];
    assert!(
        cargo_fetch.contains("cargo fetch --locked"),
        "Cargo source fetch:\n{cargo_fetch}"
    );
    for (start, end, command) in [
        (clippy, tests, "mbx clippy"),
        (tests, doctests, "mbx test"),
        (doctests, documentation, "mbx test"),
        (documentation, export, "mbx doc"),
    ] {
        assert!(
            task[start..end].contains(command),
            "MBX build command {command}:\n{}",
            &task[start..end]
        );
    }
    Ok(())
}

/// Assert explicit bundle actions and shared restore/save identity.
fn assert_mbx_bundle_actions(task: &str) -> TestResult {
    let restore = step_at(task, "Restore MBX single bundle", "bundle restore")?;
    let import = step_at(task, "Import MBX single bundle", "bundle import")?;
    let fetch = step_at(task, "Fetch Cargo sources", "source fetch")?;
    let export = step_at(task, "Export MBX single bundle", "bundle export")?;
    let save = step_at(task, "Save MBX single bundle", "bundle save")?;
    let bundle_path = "${{ runner.temp }}/mbx-single-bundle";
    let bundle_key = "${{ steps.mbx-bundle-key.outputs.primary }}";
    let bundle_restore = &task[restore..import];
    assert!(
        bundle_restore.contains("uses: actions/cache/restore@")
            && bundle_restore.contains(&format!("key: {bundle_key}"))
            && bundle_restore.contains(&format!("path: {bundle_path}")),
        "explicit bundle restore:\n{bundle_restore}"
    );
    let bundle_import = &task[import..fetch];
    assert!(
        bundle_import.contains("mbx cache import"),
        "explicit bundle import:\n{bundle_import}"
    );
    let bundle_export = &task[export..save];
    assert!(
        bundle_export.contains("mbx cache export --group")
            && bundle_export.contains("RUNNER_TEMP/mbx-single-bundle"),
        "bundle export command and path:\n{bundle_export}"
    );
    let bundle_save = &task[save..];
    assert!(
        bundle_save.contains("uses: actions/cache/save@")
            && bundle_save.contains(&format!("key: {bundle_key}"))
            && bundle_save.contains(&format!("path: {bundle_path}")),
        "explicit bundle save shares restore identity:\n{bundle_save}"
    );
    Ok(())
}

#[test]
fn w1_crate_jobs_group_obligations_per_crate() -> TestResult {
    let (_repo, yaml) = preview_yaml(config_with_branch())?;
    assert!(yaml.contains("  rust-demo:"), "one job per crate:\n{yaml}");
    assert!(
        yaml.contains("name: Rust / demo"),
        "crate display name:\n{yaml}"
    );
    assert!(!yaml.contains("velnor-task"), "no task job:\n{yaml}");
    assert!(!yaml.contains("strategy:"), "no matrix:\n{yaml}");
    assert!(!yaml.contains("fromJSON"), "no matrix ref:\n{yaml}");
    let job = window(&yaml, "  rust-demo:", "  required:")?;
    for step in [
        "Checkout",
        "Prepare pinned tools",
        "Prepare Rust components",
        "Clippy",
        "Unit and integration tests",
        "Doctests",
        "Documentation",
    ] {
        assert!(job.contains(&format!("- name: {step}")), "{step}:\n{job}");
    }
    let lint_at = job.find("- name: Clippy").ok_or("clippy")?;
    let run_at = job
        .find("- name: Unit and integration tests")
        .ok_or("run")?;
    assert!(lint_at < run_at, "clippy blocks tests:\n{job}");
    let final_gate = window(&yaml, "  required:", "  plan:")?;
    assert!(
        final_gate.contains("- rust-demo"),
        "final needs the crate:\n{final_gate}"
    );
    Ok(())
}

#[test]
fn w1_crate_job_prepares_pinned_tools() -> TestResult {
    let (_repo, yaml) = preview_yaml(config_with_branch())?;
    let task = window(&yaml, "  rust-demo:", "  required:")?;
    let checkout = task.find("- name: Checkout").ok_or("task checkout")?;
    let prepare = task
        .find("- name: Prepare pinned tools")
        .ok_or("task prepare")?;
    let run = task.find("- name: Clippy").ok_or("first obligation")?;
    assert!(checkout < prepare && prepare < run, "order:\n{task}");
    let catalog = ToolCatalog::pinned();
    assert!(
        task.contains(&format!("install {}", catalog.tool_spec(PinnedTool::Rust))),
        "install:\n{task}"
    );
    assert!(
        !task.contains("mr-boxington"),
        "cargo crate MBX-free:\n{task}"
    );
    Ok(())
}

#[test]
fn w1_crate_prepare_adds_mbx_driver() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    with_mbx(&repo)?;
    fs::write(
        repo.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let task = window(yaml, "  rust-demo:", "  required:")?;
    let catalog = ToolCatalog::pinned();
    assert!(
        task.contains(&catalog.tool_spec(PinnedTool::MrBoxington)),
        "mbx spec:\n{task}"
    );
    assert_mbx_setup(task)?;
    assert_mbx_bundle_order(task)?;
    assert_mbx_bundle_actions(task)?;
    Ok(())
}

#[test]
fn w1_crate_prepare_and_obligations_share_toolchain_union() -> TestResult {
    let (_repo, yaml) = preview_yaml(config_with_branch())?;
    let task = window(&yaml, "  rust-demo:", "  required:")?;
    let prepare_at = task.find("- name: Prepare pinned tools").ok_or("prepare")?;
    let run_at = task.find("- name: Clippy").ok_or("first obligation")?;
    let catalog = ToolCatalog::pinned();
    assert!(
        task[prepare_at..run_at].contains(&catalog.tool_spec(PinnedTool::Rust)),
        "prepare installs the driver:\n{task}"
    );
    for tool in [
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ] {
        let spec = catalog.tool_spec(tool);
        assert!(
            task[prepare_at..run_at].contains(&spec),
            "crate jobs install {spec} for test-spawned generate:\n{task}"
        );
    }
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:", "RUSTUP_TOOLCHAIN:"] {
        let line = env_line(&task[run_at..], key)?;
        assert_eq!(
            env_line(&task[prepare_at..run_at], key)?,
            line,
            "{key}:\n{task}"
        );
    }
    Ok(())
}

#[test]
fn w1_crate_cache_v1_emits_no_cache_steps() -> TestResult {
    let (_repo, yaml) = preview_yaml(config_with_branch())?;
    let task = window(&yaml, "  rust-demo:", "  required:")?;
    assert!(
        !task.contains("- name: Restore cache"),
        "v1 no restore:\n{task}"
    );
    assert!(!task.contains("- name: Save cache"), "v1 no save:\n{task}");
    Ok(())
}
