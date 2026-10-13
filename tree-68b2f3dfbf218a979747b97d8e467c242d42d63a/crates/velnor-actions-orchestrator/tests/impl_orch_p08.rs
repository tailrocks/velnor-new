//! P08 generator integration: qualified caches in emitted workflows.
//!
//! Cargo-only fixtures use pinned `rust-cache` (registry-only, shared key);
//! MBX fixtures use a local backend and explicit directory bundle, while Cargo
//! sources use one shared `actions/cache` snapshot (plan writes, crates read).
//! Tools use the built-in Mise cache only.

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, make_repo};

/// Hand-written lock for a no-deps fixture package: hermetic, no network.
pub(super) fn demo_lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// Select MBX for the fixture crate via `rustc-wrapper` evidence.
pub(super) fn with_mbx(root: &std::path::Path) -> TestResult {
    let cargo_dir = root.join(".cargo");
    fs::create_dir_all(&cargo_dir)?;
    fs::write(
        cargo_dir.join("config.toml"),
        "[build]\nrustc-wrapper = \"mbx\"\n",
    )?;
    Ok(())
}

/// Rendered workflow text for a lockful fixture (MBX when `mbx`).
pub(super) fn yaml_for(mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    if mbx {
        with_mbx(root)?;
    }
    let prep = prepare(root)?;
    let tree = render_staged_tree(&prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Step names of one IR job in a lockful fixture (MBX when `mbx`).
fn job_names(job: &str, mbx: bool) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    if mbx {
        with_mbx(root)?;
    }
    let prep = prepare(root)?;
    let found = prep
        .workflow
        .ir
        .jobs
        .get(job)
        .ok_or_else(|| std::io::Error::other(format!("missing {job}")))?;
    Ok(found.steps.iter().map(|step| step.name.clone()).collect())
}

#[test]
fn c2_builtin_mise_restore_with_elected_tools_saves() -> TestResult {
    for mbx in [false, true] {
        let yaml = yaml_for(mbx)?;
        assert!(
            !yaml.contains("Restore Mise tools"),
            "restores stay built-in (mbx={mbx})"
        );
        assert!(
            yaml.contains("- name: Save Mise tools"),
            "elected writers save (mbx={mbx})"
        );
        assert!(
            !yaml.contains("mise-tools-v1-"),
            "no role-suffixed tools keys (mbx={mbx})"
        );
        assert!(
            yaml.contains("cache_key: mise-v1-"),
            "built-in cache key (mbx={mbx})"
        );
        assert!(yaml.contains("cache: \"true\""), "built-in on (mbx={mbx})");
    }
    Ok(())
}

#[test]
fn c3_sources_subset_at_owned_home_single_writer() -> TestResult {
    let yaml = yaml_for(true)?;
    for need in [
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/git/db",
        "velnor-v1-sources-",
        "hashFiles('Cargo.lock')",
    ] {
        assert!(yaml.contains(need), "sources snapshot misses {need}");
    }
    for banned in ["credentials.toml", "registry/src"] {
        assert!(!yaml.contains(banned), "sources must exclude {banned}");
    }
    let plan = job_names("plan", true)?;
    assert!(plan.contains(&"Save Cargo sources".to_owned()), "{plan:?}");
    let crates = job_names("rust-demo", true)?;
    assert!(
        crates.contains(&"Restore Cargo sources".to_owned()),
        "{crates:?}"
    );
    assert!(
        !crates.contains(&"Save Cargo sources".to_owned()),
        "readers never save: {crates:?}"
    );
    Ok(())
}

fn rust_demo_job(yaml: &str) -> Result<&str, &'static str> {
    let start = yaml.find("  rust-demo:").ok_or("missing rust-demo job")?;
    let tail = &yaml[start..];
    Ok(&tail[..tail.find("\n  required:").unwrap_or(tail.len())])
}

fn assert_source_fetch_contract(task: &str) {
    for need in [
        "metadata --locked --offline",
        "sources hit, skipping fetch",
        "sources miss (source_missing)",
        "cargo fetch --locked",
    ] {
        assert!(task.contains(need), "rust-demo fetch/mbx misses {need}");
    }
    assert!(
        !task.contains("github-cache-mode"),
        "retired MBX cache mode field:\n{task}"
    );
}

fn step_at(task: &str, name: &str, missing: &'static str) -> Result<usize, &'static str> {
    task.find(&format!("- name: {name}")).ok_or(missing)
}

fn assert_local_mbx_backend(task: &str) -> TestResult {
    let setup = step_at(task, "Setup MBX", "MBX setup")?;
    let key = step_at(task, "Prepare MBX bundle key", "MBX bundle key")?;
    assert!(task[setup..key].contains("backend: local"), "local backend");
    Ok(())
}

fn assert_mbx_bundle_lifecycle(task: &str) -> TestResult {
    let restore = step_at(task, "Restore MBX single bundle", "bundle restore")?;
    let import = step_at(task, "Import MBX single bundle", "bundle import")?;
    let fetch = step_at(task, "Fetch Cargo sources", "Cargo fetch")?;
    let clippy = step_at(task, "Clippy", "Cargo build")?;
    let export = step_at(task, "Export MBX single bundle", "bundle export")?;
    let save = step_at(task, "Save MBX single bundle", "bundle save")?;
    assert!(
        restore < import && import < fetch && fetch < clippy && clippy < export && export < save,
        "bundle restore/import precede Cargo work; export/save follow it:\n{task}"
    );
    for (start, end, expected, label) in [
        (restore, import, "uses: actions/cache/restore@", "restore"),
        (import, fetch, "mbx cache import", "import"),
        (export, save, "mbx cache export --group", "export"),
        (save, task.len(), "uses: actions/cache/save@", "save"),
    ] {
        assert!(
            task[start..end].contains(expected),
            "explicit bundle {label}:\n{}",
            &task[start..end]
        );
    }
    Ok(())
}

#[test]
fn c4_mbx_and_restore_precede_fetch_with_offline_skip() -> TestResult {
    let names = job_names("rust-demo", true)?;
    let at = |name: &str| names.iter().position(|seen| seen == name);
    let (Some(restore), Some(mbx), Some(fetch), Some(clippy)) = (
        at("Restore Cargo sources"),
        at("Setup MBX"),
        at("Fetch Cargo sources"),
        at("Clippy"),
    ) else {
        return Err(format!("order missing: {names:?}").into());
    };
    assert!(
        restore < mbx && mbx < fetch && fetch < clippy,
        "restore<mbx<fetch<clippy: {names:?}"
    );
    let yaml = yaml_for(true)?;
    let task = rust_demo_job(&yaml)?;
    assert_source_fetch_contract(task);
    assert_local_mbx_backend(task)?;
    assert_mbx_bundle_lifecycle(task)?;
    Ok(())
}

#[test]
fn c7_cargo_only_uses_pinned_rust_cache_never_with_mbx() -> TestResult {
    let cargo_yaml = yaml_for(false)?;
    assert!(
        cargo_yaml.contains("Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6"),
        "full-SHA pin:\n{cargo_yaml}"
    );
    for need in [
        "shared-key: velnor-cargo-",
        "cache-targets: \"false\"",
        "cache-on-failure: \"false\"",
    ] {
        assert!(cargo_yaml.contains(need), "rust-cache misses {need}");
    }
    assert!(
        !cargo_yaml.contains("mr-boxington-action"),
        "cargo-only has no MBX"
    );
    let mbx_yaml = yaml_for(true)?;
    assert!(
        !mbx_yaml.contains("Swatinem/rust-cache"),
        "MBX never stacks rust-cache"
    );
    assert!(
        !mbx_yaml.contains("server-url") && !mbx_yaml.contains("backend: server"),
        "no remote MBX infra"
    );
    let plan = job_names("plan", false)?;
    assert!(
        plan.contains(&"Restore Cargo registry".to_owned()),
        "{plan:?}"
    );
    let crates = job_names("rust-demo", false)?;
    assert!(
        crates.contains(&"Restore Cargo registry".to_owned()),
        "{crates:?}"
    );
    Ok(())
}

#[test]
fn c8_crate_jobs_install_validators_for_test_spawns() -> TestResult {
    for mbx in [false, true] {
        let repo = make_repo(config_with_branch())?;
        let root = repo.path();
        fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
        if mbx {
            with_mbx(root)?;
        }
        let prep = prepare(root)?;
        let job = prep
            .workflow
            .ir
            .jobs
            .get("rust-demo")
            .ok_or_else(|| std::io::Error::other("missing crate job"))?;
        for step in &job.steps {
            if step.name != "Prepare pinned tools" {
                continue;
            }
            let StepKind::Shell { run, .. } = &step.kind else {
                continue;
            };
            let text = run.join(" ");
            for tool in ["actionlint@", "shellcheck@", "zizmor@"] {
                assert!(
                    text.contains(tool),
                    "crate prepare must install {tool} for test-spawned generate: {text}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn c10_only_plan_saves_producer_successful_deltas() -> TestResult {
    // MBX repo: plan saves after fetch; crates restore only.
    let plan = job_names("plan", true)?;
    let fetch = plan
        .iter()
        .position(|name| name == "Fetch Cargo sources")
        .ok_or("plan fetch")?;
    let save = plan
        .iter()
        .position(|name| name == "Save Cargo sources")
        .ok_or("plan save")?;
    assert!(fetch < save, "save after the writer finishes: {plan:?}");
    // Cargo-only repo: rust-cache writers save via post on success only.
    let yaml = yaml_for(false)?;
    assert!(yaml.contains("save-if: \"true\""), "writer saves");
    assert!(yaml.contains("save-if: \"false\""), "readers restore-only");
    Ok(())
}
