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

use velnor_actions_contract::{
    StepKind,
    workflow::{TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1},
};
use velnor_actions_mise::{PinnedTool, ToolCatalog, cache_sources};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{render::WORKFLOW_PATH, tree::RenderedTree};

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
    let tree = tree_for(lock, mbx)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Rendered output tree for one workspace.
fn tree_for(lock: &str, mbx: bool) -> Result<RenderedTree, Box<dyn std::error::Error>> {
    let repo = make_workspace(lock, mbx)?;
    let prep = prepare(repo.path())?;
    Ok(render_staged_tree(&prep)?)
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

/// Parse and validate the exact manifest emitted beside a workflow.
fn task_manifest(
    tree: &RenderedTree,
) -> Result<TaskExecutionManifestV1, Box<dyn std::error::Error>> {
    let marked = tree
        .get(TASK_EXECUTION_MANIFEST_PATH)
        .ok_or("missing task execution manifest")?;
    let (_, json) = marked
        .split_once('\n')
        .ok_or("missing task execution manifest marker")?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(json)?;
    manifest.validate()?;
    Ok(manifest)
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
        let tree = render_staged_tree(&prep)?;
        let manifest = task_manifest(&tree)?;
        let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
        let driver = if mbx { "mbx" } else { "cargo" };
        let mut obligations = 0;
        for (job_id, job) in &prep.workflow.ir.jobs {
            if !job_id.starts_with("rust-") {
                continue;
            }
            for step in &job.steps {
                if !["Clippy", "Build", "Test", "Nextest", "Doc", "Format"]
                    .contains(&step.name.as_str())
                {
                    continue;
                }
                assert_task_is_offline_and_manifest_bound(
                    &step.kind,
                    &manifest,
                    yaml,
                    job_id,
                    driver,
                    &ToolCatalog::pinned(),
                )?;
                obligations += 1;
            }
        }
        assert!(obligations >= 2, "obligations present (mbx={mbx})");
    }
    Ok(())
}

fn assert_task_is_offline_and_manifest_bound(
    kind: &StepKind,
    manifest: &TaskExecutionManifestV1,
    yaml: &str,
    job_id: &str,
    driver: &str,
    catalog: &ToolCatalog,
) -> Result<(), Box<dyn std::error::Error>> {
    let StepKind::TaskExecution {
        argv,
        task_id,
        task_digest,
        toolchain_inputs,
        env,
        matrix_id,
        matrix_key,
        report_helper_version,
        matrix_max_parallel,
    } = kind
    else {
        return Err("obligation must be a declared task".into());
    };
    assert!(
        argv.iter().any(|arg| arg == "--offline"),
        "{job_id} is offline: {argv:?}"
    );
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .ok_or("task command misses Mise payload separator")?;
    let payload = &argv[separator + 1..];
    let rust_selector = format!("+{}", catalog.version(PinnedTool::Rust));
    let command = match driver {
        "cargo" if payload.first().is_some_and(|arg| arg == "cargo") => &payload[1..],
        "mbx"
            if payload.first().is_some_and(|arg| arg == "mbx")
                && payload.get(1).is_some_and(|arg| arg == &rust_selector) =>
        {
            &payload[2..]
        }
        _ => {
            return Err(format!(
                "{job_id} payload does not start with its selected driver: {payload:?}"
            )
            .into());
        }
    };
    let expected_subcommand = expected_task_subcommand(task_id, &toolchain_inputs.test_runner)?;
    assert_eq!(
        command.first().map(String::as_str),
        Some(expected_subcommand),
        "{job_id}"
    );
    let record = manifest
        .tasks
        .get(task_id)
        .ok_or("task missing from manifest")?;
    assert_eq!(record.task_id, *task_id);
    assert_eq!(record.task_digest, *task_digest);
    assert_eq!(record.argv, *argv);
    assert_eq!(record.toolchain_inputs, *toolchain_inputs);
    assert_eq!(record.env, *env);
    assert_eq!(record.matrix_id, *matrix_id);
    assert_eq!(record.matrix_key, *matrix_key);
    assert_eq!(record.report_helper_version, *report_helper_version);
    assert_eq!(record.matrix_max_parallel, *matrix_max_parallel);
    assert!(
        yaml.contains(&record.execution_digest),
        "{job_id} binds its manifest record"
    );
    Ok(())
}

fn expected_task_subcommand(
    task_id: &str,
    test_runner: &str,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let kind = task_id.split('/').nth(3).ok_or("task ID misses its kind")?;
    match (kind, test_runner) {
        ("clippy", _) => Ok("clippy"),
        ("build", "cargo_test") => Ok("test"),
        ("build", "cargo_nextest") => Ok("nextest"),
        ("test", _) => Ok("test"),
        ("nextest", _) => Ok("nextest"),
        ("doctest", _) => Ok("test"),
        ("doc", _) => Ok("doc"),
        ("fmt", _) => Ok("fmt"),
        _ => Err(format!("unsupported task ID or test runner: {task_id} / {test_runner}").into()),
    }
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
