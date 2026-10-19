//! Crate-job graph regressions: one job per crate with parallel checks.
//!
//! Split from `impl_wire_w1`: generated-YAML shape, per-crate drivers,
//! obligation ordering, toolchain sharing, and cache silence.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::{
    TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{render::WORKFLOW_PATH, tree::RenderedTree};

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
    let mut limit = tail.find(end).unwrap_or(tail.len());
    let mut offset = 0;
    for (index, line) in tail.split_inclusive('\n').enumerate() {
        if index > 0
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            limit = limit.min(offset);
            break;
        }
        offset += line.len();
    }
    Ok(&tail[..limit])
}

#[test]
fn w1_crate_jobs_group_obligations_per_crate() -> TestResult {
    let (repo, yaml) = preview_yaml(config_with_branch())?;
    for obsolete in ["lint", "tests", "docs"] {
        assert!(
            !yaml.contains(&format!("  rust-demo-{obsolete}:")),
            "no per-phase job"
        );
    }
    assert!(!yaml.contains("velnor-task"), "no task job:\n{yaml}");
    assert!(!yaml.contains("strategy:"), "no matrix:\n{yaml}");
    assert!(!yaml.contains("fromJSON"), "no matrix ref:\n{yaml}");
    let prep = prepare(repo.path())?;
    let id = "rust-demo";
    let obligations = [
        "Format",
        "Clippy",
        "Unit and integration tests",
        "Doctests",
        "Documentation",
    ];
    let job = window(&yaml, &format!("  {id}:"), "  required:")?;
    assert!(job.contains("name: Rust / demo\n"), "{job}");
    for step in [
        "Checkout",
        "Prepare pinned tools",
        "Prepare Rust components",
    ] {
        assert!(job.contains(&format!("- name: {step}")), "{step}: {job}");
    }
    for step in &obligations {
        assert!(
            job.contains(&format!("- name: {step}\n        background: true")),
            "{step} runs in the parallel group: {job}"
        );
    }
    let join = job
        .find("- name: Wait for parallel checks\n        wait-all:")
        .ok_or("join")?;
    let upload = job.find("- name: Upload crate reports").ok_or("upload")?;
    assert!(join < upload, "reports upload after the join:\n{job}");
    let model = prep.workflow.ir.jobs.get(id).ok_or("crate IR")?;
    assert_eq!(model.needs, ["plan"], "no cross-phase gate owners");
    let expected: Vec<_> = model
        .steps
        .iter()
        .filter_map(|step| {
            matches!(&step.kind, StepKind::TaskExecution { .. }).then_some(step.name.as_str())
        })
        .collect();
    assert_eq!(expected, obligations, "one job owns each obligation");
    assert!(
        prep.workflow.ir.jobs["required"]
            .needs
            .iter()
            .any(|need| need == id),
        "Required owns {id}"
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
    {
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
            task.matches("Install MBX").count(),
            1,
            "one install step:\n{task}"
        );
        assert!(
            task.contains("backend: local") && !task.contains("github-cache-mode"),
            "{task}"
        );
    }
    Ok(())
}

#[test]
fn w1_crate_prepare_and_obligations_share_toolchain_union() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let task = window(yaml, "  rust-demo:", "  required:")?;
    assert_crate_job_prepares_required_tools(task)?;
    assert_clippy_task_uses_toolchain_union(&prep, &tree, task)?;
    Ok(())
}

fn assert_crate_job_prepares_required_tools(task: &str) -> TestResult {
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
    Ok(())
}

fn assert_clippy_task_uses_toolchain_union(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
    task: &str,
) -> TestResult {
    let manifest_text = tree
        .get(TASK_EXECUTION_MANIFEST_PATH)
        .ok_or("missing task execution manifest")?;
    let (_, manifest_json) = manifest_text
        .split_once('\n')
        .ok_or("missing task execution manifest marker")?;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(manifest_json)?;
    manifest.validate()?;
    let crate_job = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or("missing rust-demo IR job")?;
    let clippy = crate_job
        .steps
        .iter()
        .find(|step| step.name == "Clippy")
        .ok_or("missing Clippy task")?;
    let StepKind::TaskExecution { task_id, .. } = &clippy.kind else {
        return Err("Clippy must be a declared task".into());
    };
    let record = manifest
        .tasks
        .get(task_id)
        .ok_or("missing Clippy task manifest record")?;
    assert_task_record_matches_step(&clippy.kind, record)?;
    assert_clippy_uses_pinned_rust(record);
    assert!(
        task.contains(&record.execution_digest),
        "Clippy binds its manifest record"
    );
    Ok(())
}

fn assert_task_record_matches_step(
    step: &StepKind,
    record: &TaskExecutionManifestEntryV1,
) -> TestResult {
    let StepKind::TaskExecution {
        task_id,
        task_digest,
        argv,
        env,
        toolchain_inputs,
        matrix_id,
        matrix_key,
        report_helper_version,
        matrix_max_parallel,
    } = step
    else {
        return Err("Clippy must be a declared task".into());
    };
    assert_eq!(record.task_id, *task_id);
    assert_eq!(record.task_digest, *task_digest);
    assert_eq!(record.argv, *argv);
    assert_eq!(record.env, *env);
    assert_eq!(record.toolchain_inputs, *toolchain_inputs);
    assert_eq!(record.matrix_id, *matrix_id);
    assert_eq!(record.matrix_key, *matrix_key);
    assert_eq!(record.report_helper_version, *report_helper_version);
    assert_eq!(record.matrix_max_parallel, *matrix_max_parallel);
    Ok(())
}

fn assert_clippy_uses_pinned_rust(record: &TaskExecutionManifestEntryV1) {
    let catalog = ToolCatalog::pinned();
    let rust_spec = catalog.tool_spec(PinnedTool::Rust);
    let rust_version = catalog.rustup_toolchain();
    assert!(
        record.toolchain_inputs.tools.contains(&rust_spec),
        "Clippy task identity includes pinned Rust {rust_spec}: {:?}",
        record.toolchain_inputs.tools
    );
    assert_eq!(record.toolchain_inputs.compile_driver, "cargo");
    assert_eq!(record.toolchain_inputs.test_runner, "cargo_test");
    assert_eq!(
        record.toolchain_inputs.components,
        velnor_actions_mise::PrepareRustComponents::components()
    );
    for (key, expected) in [
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", rust_version.as_str()),
    ] {
        assert_eq!(
            record.env.get(key).map(String::as_str),
            Some(expected),
            "{key}"
        );
    }
}

#[test]
fn w1_crate_cache_v1_emits_no_cache_steps() -> TestResult {
    let (_repo, yaml) = preview_yaml(config_with_branch())?;
    {
        let task = window(&yaml, "  rust-demo:", "  required:")?;
        assert!(
            !task.contains("- name: Restore cache"),
            "v1 no restore:\n{task}"
        );
        assert!(!task.contains("- name: Save cache"), "v1 no save:\n{task}");
    }
    Ok(())
}
