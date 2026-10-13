//! Crate-job graph regressions: one ordered job per crate.
//!
//! Split from `cases::wire_w1`: generated-YAML shape, per-crate drivers,
//! obligation ordering, toolchain sharing, and cache silence.

use std::fs;

use tempfile::TempDir;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::support::{TestResult, config_with_branch, make_repo};

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
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let task = window(yaml, "  rust-demo:", "  required:")?;
    let catalog = ToolCatalog::pinned();
    assert!(
        !task.contains(&catalog.tool_spec(PinnedTool::MrBoxington)),
        "the native action, not Mise, owns MBX installation:\n{task}"
    );
    assert!(
        task.contains("uses: jdx/mr-boxington-action@")
            && task.contains(&format!(
                "version: {}",
                catalog.version(PinnedTool::MrBoxington)
            )),
        "MBX action uses the exact catalog version:\n{task}"
    );
    assert_eq!(
        task.matches("Restore MBX objects").count(),
        1,
        "one objects step:\n{task}"
    );
    assert!(
        task.contains("github-cache-mode: objects")
            || task.contains("github-cache-mode: \"objects\""),
        "{task}"
    );
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
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:"] {
        let line = env_line(&task[run_at..], key)?;
        assert_eq!(
            env_line(&task[prepare_at..run_at], key)?,
            line,
            "{key}:\n{task}"
        );
    }
    assert!(
        task.contains("RUSTUP_TOOLCHAIN: 1.98.1"),
        "job carries toolchain:\n{task}"
    );
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
