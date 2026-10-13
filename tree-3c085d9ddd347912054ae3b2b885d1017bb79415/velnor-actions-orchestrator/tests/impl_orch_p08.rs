//! P08 generator integration: qualified caches in emitted workflows.
//!
//! Cargo-only and MBX fixtures use one exact `actions/cache` sources
//! snapshot (plan writes, crates read). Tools use the runtime-qualified V2
//! Mise cache.

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, make_repo};

#[path = "impl_orch_p08_tools.rs"]
mod tools;

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

/// Rendered workflow text for a lockful fixture (MBX when `mbx`).
pub(super) fn yaml_for(mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
    let prep = preparation_for(mbx)?;
    let tree = render_staged_tree(&prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Prepare the lockful fixture used by cache assertions.
pub(super) fn preparation_for(
    mbx: bool,
) -> Result<GenerationPreparation, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    if mbx {
        with_mbx(root)?;
    }
    Ok(prepare(root)?)
}

/// Step names of one IR job in a lockful fixture (MBX when `mbx`).
pub(super) fn job_names(job: &str, mbx: bool) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let prep = preparation_for(mbx)?;
    let found = prep
        .workflow
        .ir
        .jobs
        .get(job)
        .ok_or_else(|| std::io::Error::other(format!("missing {job}")))?;
    Ok(found.steps.iter().map(|step| step.name.clone()).collect())
}

#[test]
fn c2_v2_mise_cache_restores_with_elected_tools_saves() -> TestResult {
    for mbx in [false, true] {
        let yaml = yaml_for(mbx)?;
        assert!(
            yaml.contains("- name: Restore Mise tools"),
            "V2 tools cache restores explicitly (mbx={mbx})"
        );
        assert!(
            yaml.contains("- name: Save Mise tools"),
            "elected writers save (mbx={mbx})"
        );
        assert!(!yaml.contains("mise-tools-v1-"), "V2 tool keys (mbx={mbx})");
        assert!(
            yaml.contains("key: mise-tools-v2-"),
            "runtime-qualified tools cache key (mbx={mbx})"
        );
        assert!(
            yaml.contains("outputs.enabled == 'true'"),
            "runtime gate (mbx={mbx})"
        );
        assert!(
            yaml.contains("cache: \"false\""),
            "action cache disabled (mbx={mbx})"
        );
        assert!(
            yaml.contains("- name: Restore Velnor tool seed")
                && yaml.contains(
                    "cache_key: ${{ github.event_name != 'workflow_dispatch' && 'mise-v1-"
                )
                && yaml.contains("|| '' }}"),
            "static host seed is separate and disabled for dispatch (mbx={mbx})"
        );
        let setup = yaml
            .split("- name: Setup Mise")
            .nth(1)
            .ok_or("missing Setup Mise")?;
        let setup = setup
            .lines()
            .take_while(|line| !line.starts_with("      - name: "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !setup.contains("cache_key:"),
            "runtime action cache remains disabled (mbx={mbx})"
        );
        for path in [
            "${{ runner.temp }}/velnor/rustup",
            "${{ runner.temp }}/velnor/cargo/.crates.toml",
            "${{ runner.temp }}/velnor/cargo/.crates2.json",
            "${{ runner.temp }}/velnor/cargo/bin",
        ] {
            assert!(yaml.contains(path), "V2 payload misses {path} (mbx={mbx})");
        }
    }
    Ok(())
}

#[test]
fn c4_mbx_and_restore_precede_fetch_with_offline_skip() -> TestResult {
    let names = job_names("rust-demo", true)?;
    let at = |name: &str| names.iter().position(|seen| seen == name);
    let (Some(restore), Some(mbx), Some(fetch), Some(clippy)) = (
        at("Restore Cargo sources"),
        at("Restore MBX objects"),
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
    for need in [
        "metadata --locked --offline",
        "sources hit, skipping fetch",
        "sources miss (source_missing)",
        "cargo fetch --locked",
        "github-cache-mode: objects",
    ] {
        assert!(yaml.contains(need), "fetch/mbx misses {need}");
    }
    assert!(!yaml.contains("github-cache-mode: target"), "objects only");
    Ok(())
}

#[test]
fn c7_cargo_only_uses_exact_sources_transport_without_legacy_fallback() -> TestResult {
    let cargo_yaml = yaml_for(false)?;
    for need in [
        "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
        "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
        "Restore Cargo sources",
        "Save Cargo sources",
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/git/db",
    ] {
        assert!(
            cargo_yaml.contains(need),
            "Cargo-only sources misses {need}"
        );
    }
    assert!(
        !cargo_yaml.contains("Swatinem/rust-cache"),
        "retired archive:\n{cargo_yaml}"
    );
    assert!(
        !cargo_yaml.contains("mr-boxington-action"),
        "cargo-only has no MBX"
    );
    let mbx_yaml = yaml_for(true)?;
    assert!(
        !mbx_yaml.contains("Swatinem/rust-cache"),
        "no broad fallback"
    );
    assert!(
        !mbx_yaml.contains("server-url") && !mbx_yaml.contains("backend: server"),
        "no remote MBX infra"
    );
    let plan = job_names("plan", false)?;
    assert!(
        plan.contains(&"Restore Cargo sources".to_owned()),
        "{plan:?}"
    );
    assert!(plan.contains(&"Save Cargo sources".to_owned()), "{plan:?}");
    let crates = job_names("rust-demo", false)?;
    assert!(
        crates.contains(&"Restore Cargo sources".to_owned()),
        "{crates:?}"
    );
    assert!(
        !crates.contains(&"Save Cargo sources".to_owned()),
        "reader only: {crates:?}"
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
fn c11_cache_saves_push_only_prs_and_forks_read_only() -> TestResult {
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    // IR: exactly one save step (plan writer), carrying the push-only gate;
    // every other plan step (restores, fetch, obligations) stays ungated.
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    with_mbx(root)?;
    let prep = prepare(root)?;
    let plan = prep
        .workflow
        .ir
        .jobs
        .get("plan")
        .ok_or_else(|| std::io::Error::other("missing plan"))?;
    let mut saves = 0;
    for step in &plan.steps {
        if step.name == "Save Cargo sources" {
            saves += 1;
            assert_eq!(
                step.condition.as_deref(),
                Some(CACHE_SAVE_CONDITION),
                "save must be push-gated"
            );
        } else {
            assert!(
                step.condition.is_none(),
                "only the save step is gated: {}",
                step.name
            );
        }
    }
    assert_eq!(saves, 1, "single plan writer");
    // YAML: the gate renders as the save step's `if:`, and only there; no
    // unconditional `actions/cache/save` may exist (fork read-only).
    let yaml = yaml_for(true)?;
    let save_at = yaml.find("- name: Save Cargo sources").ok_or("save step")?;
    let save_condition = yaml[save_at..].lines().nth(1).ok_or("save condition")?;
    assert!(
        save_condition.contains("success() && github.event_name == 'push'"),
        "the canonical push-only save gate already denies dispatch:\n{yaml}"
    );
    tools::assert_tools_saves_push_gated_per_key(&yaml);
    Ok(())
}
