//! T24 efficiency gates 1–4 (spec §9) as named invariant tests.
//!
//! Each gate quotes its spec line and pins the enforcing machinery.
//! Gates 5–7 live in `impl_tofu_t24_gates2` (size split); gate 7
//! (docs-only, from the §9 budget table) is derived in
//! `docs/implemented/perf-tofu-t24.md`. Owns the `#[path]` fixture
//! helper the bench/scale suites share.

#[path = "tofu_perf_fixtures_t24.rs"]
pub(crate) mod tofu_perf_fixtures_t24;

#[path = "impl_tofu_t24_gates_1_2.rs"]
mod gates_1_2;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{Job, StepKind};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, plan_internal, prepare,
};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID};

use self::tofu_perf_fixtures_t24::{
    commit_two_tofu, tofu_repo, tofu_repo_with_lock, write_public_provider_locks,
};
use crate::impl_common::{Snapshot, TestResult, snapshot};

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

/// Action `with` inputs of one named step.
fn action_with<'a>(
    job: &'a Job,
    name: &str,
) -> Result<&'a BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or(format!("missing step {name}"))?;
    match &step.kind {
        StepKind::Action { with, .. } => Ok(with),
        _ => Err(format!("{name} must be an action step").into()),
    }
}

/// Gate 3: "One repository index per snapshot, cached parsed facts
/// within that snapshot, one local-module graph traversal per needed
/// analysis; no adapter-specific whole-tree rescans or repeated
/// unbounded `rg`/`find`."
#[test]
fn gate3_single_index_single_traversal_bounded_walks() -> TestResult {
    let orch = crate_src("");
    let tofu = crate_src("../velnor-actions-tofu");
    let mut calls = Vec::new();
    for hit in token_hits(&orch, "build_file_index(")? {
        if !hit_code(&orch, &hit)?.contains("fn build_file_index") {
            calls.push(hit);
        }
    }
    assert_eq!(calls.len(), 1, "one index per snapshot: {calls:?}");
    assert!(calls[0].starts_with("discover.rs"), "{calls:?}");
    let mut selections = Vec::new();
    for hit in token_hits(&orch, "select_roots(")? {
        if !hit_code(&orch, &hit)?.contains("fn select_roots") {
            selections.push(hit);
        }
    }
    for hit in token_hits(&tofu, "select_roots(")? {
        if !hit_code(&tofu, &hit)?.contains("fn select_roots") {
            selections.push(format!("tofu/{hit}"));
        }
    }
    assert_eq!(selections.len(), 1, "one traversal entry: {selections:?}");
    assert!(
        selections[0].starts_with("select_tofu.rs"),
        "{selections:?}"
    );
    assert!(
        token_hits(&tofu, "build_file_index")?.is_empty(),
        "adapter uses the shared index"
    );
    let walks = token_hits(&tofu, "read_dir")?;
    assert_eq!(walks.len(), 1, "only bounded unit walk: {walks:?}");
    assert!(walks[0].starts_with("closure.rs"), "{walks:?}");
    let closure = std::fs::read_to_string(tofu.join("closure.rs"))?;
    assert!(
        closure.contains("MAX_FILES_PER_UNIT"),
        "unit walk stays capped"
    );
    Ok(())
}

/// Gate 4: "At most one successful initialization per selected root
/// per verification attempt; formatting visits each intended file
/// once; no duplicate provider archive uploads or unrelated tool setup."
#[test]
fn gate4_init_once_fmt_once_no_duplicate_uploads() -> TestResult {
    let dir = tofu_repo(3)?;
    write_public_provider_locks(dir.path(), &tofu_perf_fixtures_t24::tofu_root_names(3))?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let catalog = ToolCatalog::pinned();
    let rust = catalog
        .tool_spec(PinnedTool::Rust)
        .expect("qualified selector");
    let opentofu = catalog
        .native_tool_spec(
            velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
            PinnedTool::Opentofu,
        )
        .expect("qualified selector");
    let ids: Vec<&String> = jobs
        .iter()
        .filter(|(id, job)| is_crate_job(id) && job.source_producer.is_none())
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    let mut fmt_tasks = Vec::new();
    for id in &ids {
        let job = jobs.get(*id).ok_or("missing crate job")?;
        let steps = names(job);
        assert_eq!(
            steps
                .iter()
                .filter(|name| **name == "Init for validate")
                .count(),
            1,
            "{id} inits once: {steps:?}"
        );
        let (_, fmt_env) = shell_of(job, "Format")?;
        let task = fmt_env.get("VELNOR_TASK_ID").ok_or("fmt task id")?.clone();
        assert!(
            task.starts_with("stack/tofu/") && task.ends_with("/fmt/default"),
            "{id} fmt binds its scope: {task}"
        );
        fmt_tasks.push(task);
        let (run, _) = shell_of(job, "Prepare pinned tools")?;
        assert!(run.contains(&opentofu), "{id}: {run:?}");
        assert!(!run.contains(&rust), "{id} keeps no rust setup: {run:?}");
    }
    fmt_tasks.sort();
    fmt_tasks.dedup();
    assert_eq!(fmt_tasks.len(), 3, "each scope formats once");
    let mut saves = Vec::new();
    for (id, job) in &jobs {
        let count = names(job)
            .iter()
            .filter(|name| **name == "Save Tofu providers")
            .count();
        if job.source_producer.is_some() {
            assert_eq!(count, 1, "{id} producer saves once");
            let with = action_with(job, "Save Tofu providers")?;
            saves.push(with.get("key").ok_or("save key")?.clone());
        } else {
            assert_eq!(count, 0, "{id} consumer never saves");
        }
    }
    assert_eq!(saves.len(), 3, "one producer upload per root");
    saves.sort();
    saves.dedup();
    assert_eq!(saves.len(), 3, "no duplicate provider archive upload");
    Ok(())
}
