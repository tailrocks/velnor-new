//! P08 generator integration: qualified caches in emitted workflows.
//!
//! Cargo-only fixtures use pinned `rust-cache` (registry-only, shared key);
//! MBX fixtures use objects + one shared `actions/cache` snapshot (plan
//! writes, crates read). Tools use explicit canonical Mise state archives.

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
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

/// Rendered workflow text for a lockful fixture (MBX when `mbx`).
fn yaml_for(mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
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
fn c2_explicit_mise_restore_with_elected_tools_saves() -> TestResult {
    for mbx in [false, true] {
        let yaml = yaml_for(mbx)?;
        assert!(
            yaml.contains("- name: Restore Mise tools"),
            "explicit restore runs before setup (mbx={mbx})"
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
            yaml.contains("key: mise-v3-${{ runner.os }}-${{ runner.arch }}-${{ env.VELNOR_CACHE_IMAGE_OS }}-${{ env.VELNOR_CACHE_IMAGE_VERSION }}-"),
            "explicit cache has canonical image-aware key (mbx={mbx})"
        );
        assert!(
            yaml.contains("cache: \"false\""),
            "built-in off (mbx={mbx})"
        );
        assert!(
            !yaml.contains("cache_key:"),
            "implicit cache key disabled (mbx={mbx})"
        );
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
        !plan.contains(&"Restore Cargo registry".to_owned()),
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

#[test]
fn c11_cache_saves_protected_default_prs_and_forks_read_only() -> TestResult {
    // IR: exactly one save step (plan writer), carrying the protected-default gate;
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
                "save must use the protected-default predicate"
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
    let expected_save = format!(
        "- name: Save Cargo sources\n        if: {}",
        velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION
    );
    assert!(
        yaml[save_at..].starts_with(&expected_save),
        "save renders protected-default predicate:\n{yaml}"
    );
    assert_tools_saves_protected_default_per_key(&yaml);
    Ok(())
}

/// YAML: one trusted tools save per restored `mise-v3-` key (plus the
/// sources save), every setup restore-only.
fn assert_tools_saves_protected_default_per_key(yaml: &str) {
    let mut keys = std::collections::BTreeSet::new();
    let mut in_tools_restore = false;
    for line in yaml.lines() {
        if let Some(name) = line.trim().strip_prefix("- name: ") {
            in_tools_restore = name == "Restore Mise tools";
        }
        if in_tools_restore && let Some(key) = line.trim().strip_prefix("key: ") {
            keys.insert(key.trim_matches('"').to_owned());
            in_tools_restore = false;
        }
    }
    assert!(!keys.is_empty(), "at least one restored tools key:\n{yaml}");
    let mbx_saves = yaml.matches("- name: Save MBX single bundle").count();
    assert_eq!(
        yaml.matches("actions/cache/save@").count(),
        1 + keys.len() + mbx_saves,
        "sources plus one tools save per key plus the MBX bundle:\n{yaml}"
    );
    assert_eq!(
        yaml.matches(&format!("if: {CACHE_SAVE_CONDITION}")).count(),
        1 + keys.len(),
        "source and tool cache producers use the shared trusted-save predicate:\n{yaml}"
    );
    assert_eq!(
        yaml.matches(&format!(
            "if: {CACHE_SAVE_CONDITION} && env.VELNOR_CACHE_IMAGE_ELIGIBLE == 'true'"
        ))
        .count(),
        keys.len(),
        "every tool save also requires a qualified runner image:\n{yaml}"
    );
    assert!(
        !yaml.contains("- name: Restore Cargo sources\n        if:"),
        "restores stay unconditional:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("- name: Save Mise tools").count(),
        keys.len(),
        "exactly one tools saver per key:\n{yaml}"
    );
    for line in yaml.lines() {
        if let Some(key) = line.trim().strip_prefix("key: ")
            && key.starts_with("mise-v3-")
        {
            assert!(
                keys.contains(key),
                "tools save archives a restored key:\n{yaml}"
            );
        }
    }
    assert!(
        !yaml.contains("cache_save: \"true\""),
        "no unconditional mise save:\n{yaml}"
    );
    assert!(
        !yaml.contains("cache_save: ${{"),
        "no promised built-in save:\n{yaml}"
    );
    let setups = yaml.matches("- name: Setup Mise").count();
    let demoted = yaml.matches("cache_save: \"false\"").count();
    assert_eq!(setups, demoted, "every setup restore-only:\n{yaml}");
}
