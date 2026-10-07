//! T18 root grouping end to end: one job per validation root,
//! fmt/init/validate as same-job ordered steps, fmt once per scope,
//! init once per root, and `max_parallel_jobs` lane staging.
use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract_workflow::{Job, StepKind};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_jobs::context::{FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, git, install_fixture_release_manifest};

/// True for generated crate jobs (neither plan, lint, gate, nor publish).
fn is_crate_job(id: &str) -> bool {
    id != PLAN_JOB_ID && id != "actionlint" && id != FINAL_JOB_ID && id != PUBLISH_JOB_ID
}

/// YAML slice of one rendered job: from its `  <id>:` header line to
/// the next two-space job header (or end of jobs).
fn job_window<'a>(yaml: &'a str, id: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let header = format!("  {id}:");
    let from = yaml.find(&header).ok_or("job header")?;
    let mut end = yaml.len();
    let mut at = from + header.len();
    for line in yaml[at..].split_inclusive('\n') {
        let trimmed = line.trim_end();
        let Some(content) = trimmed.strip_prefix("  ") else {
            at += line.len();
            continue;
        };
        if !content.starts_with(' ')
            && content.len() > 1
            && content.ends_with(':')
            && !content.contains(' ')
        {
            end = at;
            break;
        }
        at += line.len();
    }
    Ok(&yaml[from..end])
}

/// Step names of one finalized job.
fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

/// Shell env of one named step.
fn shell_env_of<'a>(
    job: &'a Job,
    name: &str,
) -> Result<&'a BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or(format!("missing step {name}"))?;
    match &step.kind {
        StepKind::Shell { env, .. } => Ok(env),
        _ => Err(format!("{name} must be a shell step").into()),
    }
}

/// Git-initialized pure-tofu repo: `config` plus `files`, no Cargo.
fn make_pure_tofu_repo(
    config: &str,
    files: &[(&str, &str)],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    install_fixture_release_manifest(root)?;
    for (relative, content) in files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    Ok(dir)
}

/// Minimal lockfile so the fixture root counts as lockful.
fn demo_lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// Three disjoint roots with non-empty fmt scopes, capped 2-wide.
fn three_root_config() -> String {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nmax_parallel_jobs = 2\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\", \"stacks/c\"]\n"
        .to_owned()
}

fn three_root_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("stacks/a/main.tf", "variable \"a\" {}\n"),
        ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ("stacks/c/main.tf", "variable \"c\" {}\n"),
    ]
}

/// Sorted crate-job IDs of one finalized job map.
fn sorted_crate_ids(jobs: &BTreeMap<String, Job>) -> Vec<String> {
    let mut ids: Vec<String> = jobs.keys().filter(|id| is_crate_job(id)).cloned().collect();
    ids.sort();
    ids
}

/// Three roots emit three jobs; each carries fmt/init/validate once,
/// in order, bound to its own root.
#[test]
fn one_job_per_validation_root_with_ordered_steps() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    let mut roots = Vec::new();
    for id in &ids {
        let job = jobs.get(id).ok_or("missing crate job")?;
        let steps = names(job);
        let fmt_at = steps
            .iter()
            .position(|name| *name == "Format")
            .ok_or(format!("{id} without Format: {steps:?}"))?;
        let init_at = steps
            .iter()
            .position(|name| *name == "Init for validate")
            .ok_or(format!("{id} without init: {steps:?}"))?;
        let validate_at = steps
            .iter()
            .position(|name| *name == "Validate")
            .ok_or(format!("{id} without validate: {steps:?}"))?;
        assert!(
            fmt_at < init_at && init_at < validate_at,
            "{id} orders fmt < init < validate: {steps:?}"
        );
        assert_eq!(
            steps.iter().filter(|name| **name == "Validate").count(),
            1,
            "{id} validates once: {steps:?}"
        );
        let env = shell_env_of(job, "Validate")?;
        let task_id = env.get("VELNOR_TASK_ID").ok_or("validate task id")?;
        assert!(
            task_id.starts_with("stack/tofu/") && task_id.ends_with("/validate/default"),
            "{id} validate binds its root: {task_id}"
        );
        roots.push(task_id.clone());
    }
    roots.sort();
    roots.dedup();
    assert_eq!(roots.len(), 3, "each job binds its own root");
    Ok(())
}

/// Staging needs chain lanes: at most `max_parallel_jobs` root jobs
/// run concurrently (job[i] waits for job[i - max]).
#[test]
fn root_jobs_stage_needs_by_max_parallel() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    assert_eq!(
        jobs[&ids[0]].needs,
        vec![PLAN_JOB_ID.to_owned()],
        "first lane needs plan only"
    );
    assert_eq!(
        jobs[&ids[1]].needs,
        vec![PLAN_JOB_ID.to_owned()],
        "second lane needs plan only"
    );
    assert_eq!(
        jobs[&ids[2]].needs,
        vec![PLAN_JOB_ID.to_owned(), ids[0].clone()],
        "third job waits for the first lane"
    );
    Ok(())
}

/// Rendered root jobs declare the cap as `max-parallel` while the
/// marker trio itself never reaches the YAML.
#[test]
fn root_jobs_render_capped_strategy_without_markers() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let prep = prepare(dir.path())?;
    let jobs = finalized_jobs(&prep)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    for id in &ids {
        let window = job_window(yaml, id)?;
        assert!(
            window.contains("max-parallel: 2"),
            "{id} declares the configured cap:\n{window}"
        );
    }
    for marker in [
        "VELNOR_MATRIX_NEEDS_JOB",
        "VELNOR_MATRIX_OUTPUT",
        "VELNOR_MATRIX_MAX_PARALLEL",
    ] {
        assert!(
            !yaml.contains(marker),
            "marker {marker} never renders:\n{yaml}"
        );
    }
    Ok(())
}

/// Exactly one init proposal per root, each with a distinct task ID.
#[test]
fn init_runs_once_per_root() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let prep = prepare(dir.path())?;
    let mut inits: Vec<&str> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| task.stack_id == "tofu" && task.task_kind == "init")
        .map(|task| task.task_id.as_str())
        .collect();
    inits.sort_unstable();
    assert_eq!(
        inits,
        vec![
            "stack/tofu/stacks/a/init/default",
            "stack/tofu/stacks/b/init/default",
            "stack/tofu/stacks/c/init/default",
        ],
        "one init per root"
    );
    Ok(())
}

/// Nested roots format once at the outer scope: the covered inner fmt
/// carries `no_targets` while both roots keep init and validate.
#[test]
fn nested_roots_format_once_at_outer_scope() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\".\", \"stacks/a\"]\n";
    let dir = make_pure_tofu_repo(
        config,
        &[
            ("main.tf", "variable \"top\" {}\n"),
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
        ],
    )?;
    let prep = prepare(dir.path())?;
    let fmt_of = |unit: &str| {
        prep.discovery
            .proposals
            .iter()
            .find(|task| {
                task.stack_id == "tofu" && task.task_kind == "fmt" && task.identity.unit_id == unit
            })
            .map(|task| task.no_targets)
    };
    assert_eq!(fmt_of("root"), Some(false), "outer fmt runs");
    assert_eq!(fmt_of("stacks/a"), Some(true), "covered inner fmt skips");
    for unit in ["root", "stacks/a"] {
        for kind in ["init", "validate"] {
            let runnable = prep
                .discovery
                .proposals
                .iter()
                .find(|task| {
                    task.stack_id == "tofu"
                        && task.task_kind == kind
                        && task.identity.unit_id == unit
                })
                .is_some_and(|task| !task.no_targets);
            assert!(runnable, "{unit} {kind} stays runnable");
        }
    }
    let jobs = finalized_jobs(&prep)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 2, "one job per root: {ids:?}");
    let mut saw_outer_fmt = false;
    let mut saw_inner_without_fmt = false;
    for id in &ids {
        let job = jobs.get(id).ok_or("missing crate job")?;
        let env = shell_env_of(job, "Validate")?;
        let task_id = env.get("VELNOR_TASK_ID").ok_or("validate task id")?;
        let steps = names(job);
        if task_id.starts_with("stack/tofu/root/") {
            saw_outer_fmt = steps.contains(&"Format");
        } else if task_id.starts_with("stack/tofu/stacks/a/") {
            saw_inner_without_fmt = !steps.contains(&"Format");
        }
    }
    assert!(saw_outer_fmt, "outer job formats its scope");
    assert!(saw_inner_without_fmt, "covered inner job skips its fmt");
    Ok(())
}

/// Step order Restore < Init < Validate: a lockful mixed repo
/// restores before obligations while tofu orders fmt < init.
///
/// Tofu carries no restore step yet (provider cache is T21); the
/// shared construction emits restore before the obligation loop, so
/// this pins the order live on both stacks today.
#[test]
fn restore_precedes_init_precedes_validate() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n";
    let dir = make_pure_tofu_repo(
        config,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("src/lib.rs", "pub fn f() {}\n"),
            ("Cargo.lock", &demo_lock("demo")),
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
        ],
    )?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 2, "rust plus tofu jobs: {ids:?}");
    for id in &ids {
        let job = jobs.get(id).ok_or("missing crate job")?;
        let steps = names(job);
        let first_obligation = steps
            .iter()
            .position(|name| ["Format", "Clippy", "Init for validate", "Validate"].contains(name))
            .ok_or(format!("{id} without obligations: {steps:?}"))?;
        for (at, name) in steps.iter().enumerate() {
            if name.starts_with("Restore") {
                assert!(
                    at < first_obligation,
                    "{id} restores before obligations: {steps:?}"
                );
            }
        }
        let init_at = steps.iter().position(|name| *name == "Init for validate");
        let validate_at = steps.iter().position(|name| *name == "Validate");
        if let (Some(init_at), Some(validate_at)) = (init_at, validate_at) {
            assert!(
                init_at < validate_at,
                "{id} inits before validating: {steps:?}"
            );
        }
    }
    Ok(())
}

/// Staged jobs carry `if: always()` so lane failures collect instead
/// of skipping later lanes; first-in-lane jobs keep the default.
#[test]
fn staged_root_jobs_carry_always_while_first_lane_stays_default() -> TestResult {
    let dir = make_pure_tofu_repo(&three_root_config(), &three_root_files())?;
    let prep = prepare(dir.path())?;
    let jobs = finalized_jobs(&prep)?;
    let ids = sorted_crate_ids(&jobs);
    assert_eq!(ids.len(), 3, "one job per root: {ids:?}");
    assert_eq!(jobs[&ids[0]].condition, None, "first lane default");
    assert_eq!(jobs[&ids[1]].condition, None, "second lane default");
    assert_eq!(
        jobs[&ids[2]].condition.as_deref(),
        Some("always()"),
        "staged job collects lane failures"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    // Job-level `if:` renders at four spaces; the always-on report
    // upload step carries its own deeper `if: always()` in every job.
    let window = job_window(yaml, &ids[2])?;
    assert!(
        window.contains("\n    if: always()\n"),
        "staged job renders the condition: {window}"
    );
    for id in &ids[..2] {
        assert!(
            !job_window(yaml, id)?.contains("\n    if: always()\n"),
            "{id} renders no job-level condition"
        );
    }
    Ok(())
}
