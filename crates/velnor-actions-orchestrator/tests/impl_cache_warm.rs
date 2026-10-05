//! P08 warm-reuse proof: identical renders reuse, deltas re-key.
//!
//! Renders the same two-crate workspace twice from scratch and asserts the
//! warm-run contract: identical inputs produce identical cache keys (so a
//! warm runner restores instead of fetching), the fetch step carries the
//! offline-skip branch, and lockfile deltas change the cache identity.
//! Lock CONTENT deltas re-key at runtime via `hashFiles` (same template,
//! different resolved key); the hosted seed/warm pair in
//! `docs/implemented/performance.md` shows both sides of that branch.

use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_mise::cache_sources;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git};

#[path = "impl_cache_warm_components.rs"]
mod components_tests;

/// Two-crate workspace repo; `lock` selects the lockfile body.
fn make_workspace(lock: &str, mbx: bool) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"members/a\", \"members/b\"]\n",
    )?;
    for member in ["a", "b"] {
        let dir = root.join("members").join(member);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    fs::write(root.join("Cargo.lock"), lock)?;
    if mbx {
        let cargo_dir = root.join(".cargo");
        fs::create_dir_all(&cargo_dir)?;
        fs::write(
            cargo_dir.join("config.toml"),
            "[build]\nrustc-wrapper = \"mbx\"\n",
        )?;
    }
    Ok(dir)
}

/// Lockfile body pinning `members`.
fn lock_for(members: &[&str]) -> String {
    let mut body = "version = 4\n".to_owned();
    for member in members {
        body.push_str("\n[[package]]\nname = \"");
        body.push_str(member);
        body.push_str("\"\nversion = \"0.1.0\"\n");
    }
    body
}

/// Rendered workflow bytes for one workspace.
fn yaml_for(lock: &str, mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_workspace(lock, mbx)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Every `key:`/`cache_key:`/`shared-key:` value in render order.
fn cache_keys(yaml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in yaml.lines() {
        let trimmed = line.trim();
        for prefix in ["key: ", "cache_key: ", "shared-key: "] {
            if let Some(value) = trimmed.strip_prefix(prefix) {
                keys.push(value.trim_matches('"').to_owned());
            }
        }
    }
    keys
}

#[test]
fn identical_inputs_render_identical_caches() -> TestResult {
    for mbx in [false, true] {
        let lock = lock_for(&["a", "b"]);
        let first = yaml_for(&lock, mbx)?;
        let second = yaml_for(&lock, mbx)?;
        assert_eq!(first, second, "byte-identical render (mbx={mbx})");
        let keys = cache_keys(&first);
        assert!(!keys.is_empty(), "caches present (mbx={mbx})");
        assert_eq!(keys, cache_keys(&second), "identical keys (mbx={mbx})");
    }
    Ok(())
}

fn driver_task(line: &str, driver: &str, task: &str) -> bool {
    let needle = format!("{driver} {task}");
    line.match_indices(&needle).any(|(at, _)| {
        line[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric())
    })
}

#[test]
fn fetch_carries_offline_skip_branch() -> TestResult {
    for mbx in [false, true] {
        let repo = make_workspace(&lock_for(&["a", "b"]), mbx)?;
        let prep = prepare(repo.path())?;
        let mut seen_probe = false;
        for (id, job) in &prep.workflow.ir.jobs {
            if !id.starts_with("rust-") {
                continue;
            }
            for step in &job.steps {
                let StepKind::Shell { run, .. } = &step.kind else {
                    continue;
                };
                let script = run.join(" ");
                if step.name == "Fetch Cargo sources" {
                    for need in [
                        "metadata --locked --offline",
                        "sources hit, skipping fetch",
                        "sources miss (source_missing)",
                        "cargo fetch --locked",
                    ] {
                        assert!(script.contains(need), "{id} fetch misses {need}");
                    }
                    seen_probe = true;
                }
            }
        }
        assert!(seen_probe, "crate fetch probe present (mbx={mbx})");
        let yaml = yaml_for(&lock_for(&["a", "b"]), mbx)?;
        let driver = if mbx { "mbx" } else { "cargo" };
        let mut obligations = 0;
        for line in yaml.lines().filter(|line| line.contains("run:")) {
            let is_obligation = ["clippy", "build", "test", "nextest", "doc", "fmt"]
                .iter()
                .any(|task| driver_task(line, driver, task));
            if is_obligation {
                obligations += 1;
                assert!(line.contains("--offline"), "offline obligation: {line}");
            }
        }
        assert!(obligations >= 2, "obligations present (mbx={mbx})");
    }
    Ok(())
}

#[test]
fn probe_outcome_maps_to_skip_or_explicit_fetch() {
    assert_eq!(
        cache_sources::fetch_decision(true, "no_entry").expect("skip"),
        cache_sources::FetchDecision::OfflineSkip
    );
    for reason in [
        "no_entry",
        "source_missing",
        "cache_unavailable",
        "cache_corrupt",
    ] {
        assert_eq!(
            cache_sources::fetch_decision(false, reason).expect("fetch"),
            cache_sources::FetchDecision::ExplicitFetch {
                miss_reason: reason
            },
            "{reason} fetches explicitly"
        );
    }
    assert!(cache_sources::fetch_decision(false, "bogus").is_err());
}

#[test]
fn lockfile_delta_changes_cache_identity() -> TestResult {
    let lock = lock_for(&["a", "b"]);
    // Compiler driver does not change the exact Cargo sources archive.
    let mbx_keys = cache_keys(&yaml_for(&lock, true)?);
    let cargo_keys = cache_keys(&yaml_for(&lock, false)?);
    assert!(
        mbx_keys
            .iter()
            .any(|key| key.contains("velnor-v1-sources-")),
        "mbx snapshot: {mbx_keys:?}"
    );
    assert!(
        cargo_keys
            .iter()
            .any(|key| key.contains("velnor-v1-sources-")),
        "cargo sources key: {cargo_keys:?}"
    );
    assert_eq!(
        mbx_keys
            .iter()
            .find(|key| key.contains("velnor-v1-sources-")),
        cargo_keys
            .iter()
            .find(|key| key.contains("velnor-v1-sources-")),
        "same source identity in both driver modes"
    );
    // Lock CONTENT deltas re-key at runtime through `hashFiles`: the
    // template is stable across renders, the resolved key is not.
    for key in &mbx_keys {
        if key.contains("velnor-v1-sources-") {
            assert!(key.contains("hashFiles("), "content-pinned: {key}");
        }
    }
    // Lockless emits no sources cache at all: removing the lock removes reuse.
    let repo = make_workspace(&lock, true)?;
    fs::remove_file(repo.path().join("Cargo.lock"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    assert!(
        !yaml.contains("Restore Cargo sources"),
        "lockless restores nothing"
    );
    assert!(
        !yaml.contains("Save Cargo sources"),
        "lockless saves nothing"
    );
    Ok(())
}
