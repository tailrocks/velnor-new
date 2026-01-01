//! T24 efficiency gates 1–4 (spec §9) as named invariant tests.
//!
//! Each gate quotes its spec line and pins the enforcing machinery.
//! Gates 5–7 live in `cases::tofu_t24_gates2` (size split); gate 7
//! (docs-only, from the §9 budget table) is derived in
//! `docs/content/docs/implemented/perf-tofu-t24.mdx`. Owns the fixture submodule
//! the bench/scale suites share.

pub(crate) mod tofu_perf_fixtures_t24;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use velnor_actions_contract_workflow::{Job, StepKind};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, plan_internal, prepare,
};
use velnor_actions_workflow_jobs::context::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID};

use self::tofu_perf_fixtures_t24::{commit_two_tofu, tofu_repo, tofu_repo_with_lock};
use crate::support::{Snapshot, TestResult, snapshot};

/// `src/` dir of one workspace crate under test.
pub(crate) fn crate_src(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(name)
        .join("src")
}

/// Strip a trailing `//` comment, ignoring `//` inside string literals.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut escape = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'/' && bytes[index + 1] == b'/' && !quoted {
            return line[..index].trim_end();
        }
        index += 1;
    }
    line
}

/// Code body of one `name:line` hit (comments stripped).
pub(crate) fn hit_code(dir: &Path, hit: &str) -> Result<String, Box<dyn std::error::Error>> {
    let (name, line) = hit.split_once(':').unwrap_or(("", "0"));
    let want: usize = line.parse().unwrap_or(0);
    let text = std::fs::read_to_string(dir.join(name))?;
    for (number, body) in text.lines().enumerate() {
        if number + 1 == want {
            return Ok(strip_line_comment(body).to_owned());
        }
    }
    Err(format!("missing {hit}").into())
}

/// Every `name:line` holding `token` in product `.rs` files under `dir`
/// (`*_tests.rs` companions excluded; comments stripped).
pub(crate) fn token_hits(
    dir: &Path,
    token: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        entries.push(entry?.path());
    }
    entries.sort();
    let mut hits = Vec::new();
    for path in entries {
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.ends_with("_tests.rs") {
            continue;
        }
        for (number, line) in std::fs::read_to_string(&path)?.lines().enumerate() {
            if strip_line_comment(line).contains(token) {
                hits.push(format!("{name}:{}", number + 1));
            }
        }
    }
    Ok(hits)
}

/// True for generated crate jobs (neither plan, lint, gate, nor publish).
pub(crate) fn is_crate_job(id: &str) -> bool {
    id != PLAN_JOB_ID && id != "actionlint" && id != FINAL_JOB_ID && id != PUBLISH_JOB_ID
}

/// Step names of one finalized job.
pub(crate) fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

/// Borrowed shell argv+env of one step.
type Shell<'a> = (&'a Vec<String>, &'a BTreeMap<String, String>);

/// Shell argv+env of one named step.
fn shell_of<'a>(job: &'a Job, name: &str) -> Result<Shell<'a>, Box<dyn std::error::Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or(format!("missing step {name}"))?;
    match &step.kind {
        StepKind::Shell { run, env } => Ok((run, env)),
        _ => Err(format!("{name} must be a shell step").into()),
    }
}

/// Gate 1: "A pure-tofu consumer using a prebuilt generator
/// installs/runs no Rust toolchain, Cargo metadata, MBX, Nextest,
/// rustfmt, or Clippy for its stack or Plan job."
#[test]
fn gate1_pure_tofu_installs_no_rust_toolchain() -> TestResult {
    let dir = tofu_repo(2)?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let catalog = ToolCatalog::pinned();
    let rust = catalog.tool_spec(PinnedTool::Rust);
    let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
    let opentofu = catalog.tool_spec(PinnedTool::Opentofu);
    let mut checked = 0;
    for (id, job) in &jobs {
        if id != PLAN_JOB_ID && !is_crate_job(id) {
            continue;
        }
        checked += 1;
        let steps = names(job);
        assert!(
            !steps.contains(&"Prepare Rust components"),
            "{id} has no components step: {steps:?}"
        );
        assert!(
            !steps.iter().any(|name| name.starts_with("Fetch Cargo")),
            "{id} has no cargo fetch: {steps:?}"
        );
        for name in &steps {
            for banned in [
                "Rust", "Cargo", "rustfmt", "Clippy", "Nextest", "MBX", "metadata",
            ] {
                assert!(!name.contains(banned), "{id} step {name} names {banned}");
            }
        }
        let (run, env) = shell_of(job, "Prepare pinned tools")?;
        assert!(run.contains(&opentofu), "{id} installs opentofu: {run:?}");
        assert!(!run.contains(&rust), "{id} installs no Rust: {run:?}");
        assert!(!run.contains(&mbx), "{id} installs no MBX: {run:?}");
        for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
            assert!(!env.contains_key(key), "{id} prepare carries no {key}");
        }
    }
    assert_eq!(checked, 3, "plan plus two stack jobs");
    Ok(())
}

/// The tofu adapter spawns no processes and emits argv, never shell.
fn assert_tofu_zero_spawn(tofu: &Path) -> TestResult {
    for token in [
        "Command::new",
        "process::Command",
        ".spawn(",
        ".output(",
        "tokio::process",
        "std::process",
    ] {
        assert!(
            token_hits(tofu, token)?.is_empty(),
            "{token}: {:?}",
            token_hits(tofu, token)?
        );
    }
    assert!(
        token_hits(tofu, "\"sh\"")?.is_empty(),
        "tofu emits argv, never shell"
    );
    Ok(())
}

/// Discovery/plan files download nothing: merge-time retrieval
/// (`retrieve_*`) may retry downloads, but discovery never does.
fn assert_discovery_downloads_nothing(orch: &Path, tofu: &Path) -> TestResult {
    let plan_path = [
        "discover.rs",
        "discover_index.rs",
        "discover_tofu.rs",
        "select.rs",
        "select_tofu.rs",
        "internal.rs",
        "internal_plan.rs",
        "internal_request.rs",
    ];
    for token in ["download_with_retry", "reqwest", "ureq"] {
        let hits: Vec<String> = token_hits(orch, token)?
            .into_iter()
            .filter(|hit| {
                plan_path
                    .iter()
                    .any(|name| hit.starts_with(&format!("{name}:")))
            })
            .collect();
        assert!(hits.is_empty(), "{token}: {hits:?}");
    }
    for token in ["download", "fetch("] {
        assert!(
            token_hits(tofu, token)?.is_empty(),
            "{token}: {:?}",
            token_hits(tofu, token)?
        );
    }
    Ok(())
}

/// Gate 2: "`plan`/`generate` perform zero tofu init/validate/apply
/// operations, no provider downloads for discovery, and no
/// modifications to source/tool/lock files."
#[test]
fn gate2_plan_and_generate_run_zero_tofu_operations() -> TestResult {
    let tofu = crate_src("../../adapters/velnor-actions-tofu");
    assert_tofu_zero_spawn(&tofu)?;
    assert_discovery_downloads_nothing(&crate_src(""), &tofu)?;
    let dir = tofu_repo_with_lock()?;
    let root = dir.path();
    let (base, head) = commit_two_tofu(root, "stacks/a/main.tf", "variable \"bump\" {}\n")?;
    let before = snapshot(root)?;
    let request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": base, "head": head,
        "event": "pull_request", "root": root.display().to_string(),
    });
    plan_internal(&request.to_string())?;
    let prep = prepare(root)?;
    let out = tempfile::TempDir::new()?;
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(out.path().join("gate2")),
        },
    )?;
    let after = snapshot(root)?;
    let bytes = |snap: Snapshot| {
        snap.into_iter()
            .filter(|(path, _)| !path.starts_with(".git/"))
            .map(|(path, (body, _))| (path, body))
            .collect::<BTreeMap<_, _>>()
    };
    let (before, after) = (bytes(before), bytes(after));
    let mut changed = Vec::new();
    for path in before.keys().chain(after.keys()) {
        if before.get(path) != after.get(path) {
            changed.push(path.clone());
        }
    }
    changed.sort();
    changed.dedup();
    assert!(changed.is_empty(), "plan+generate modified: {changed:?}");
    Ok(())
}

/// Gate 3: "One repository index per snapshot, cached parsed facts
/// within that snapshot, one local-module graph traversal per needed
/// analysis; no adapter-specific whole-tree rescans or repeated
/// unbounded `rg`/`find`."
#[test]
fn gate3_single_index_single_traversal_bounded_walks() -> TestResult {
    let orch = crate_src("");
    let disc = crate_src("../velnor-actions-orchestrator-discovery");
    let tofu = crate_src("../../adapters/velnor-actions-tofu");
    let mut calls = Vec::new();
    for (dir, prefix) in [(&orch, ""), (&disc, "discovery/")] {
        for hit in token_hits(dir, "build_file_index(")? {
            if !hit_code(dir, &hit)?.contains("fn build_file_index") {
                calls.push(format!("{prefix}{hit}"));
            }
        }
    }
    assert_eq!(calls.len(), 1, "one index per snapshot: {calls:?}");
    assert!(calls[0].starts_with("discovery/discover.rs"), "{calls:?}");
    let mut selections = Vec::new();
    for (dir, prefix) in [(&orch, ""), (&disc, "discovery/")] {
        for hit in token_hits(dir, "select_roots(")? {
            if !hit_code(dir, &hit)?.contains("fn select_roots") {
                selections.push(format!("{prefix}{hit}"));
            }
        }
    }
    for hit in token_hits(&tofu, "select_roots(")? {
        if !hit_code(&tofu, &hit)?.contains("fn select_roots") {
            selections.push(format!("tofu/{hit}"));
        }
    }
    assert_eq!(selections.len(), 1, "one traversal entry: {selections:?}");
    assert!(
        selections[0].starts_with("discovery/select_tofu.rs"),
        "{selections:?}"
    );
    let tofu_core = crate_src("../../adapters/velnor-actions-tofu-core");
    assert!(
        token_hits(&tofu, "build_file_index")?.is_empty()
            && token_hits(&tofu_core, "build_file_index")?.is_empty(),
        "adapter uses the shared index"
    );
    assert!(
        token_hits(&tofu, "read_dir")?.is_empty(),
        "selection layer performs no walks"
    );
    let walks = token_hits(&tofu_core, "read_dir")?;
    assert_eq!(walks.len(), 1, "only bounded unit walk: {walks:?}");
    assert!(walks[0].starts_with("file_cache.rs"), "{walks:?}");
    let cache = std::fs::read_to_string(tofu_core.join("file_cache.rs"))?;
    assert!(
        cache.contains("MAX_FILES_PER_UNIT"),
        "unit walk stays capped"
    );
    Ok(())
}
