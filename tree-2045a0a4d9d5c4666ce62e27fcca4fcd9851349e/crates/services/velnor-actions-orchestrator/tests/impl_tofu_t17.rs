//! T17 per-role tools end to end: pure-tofu repos drop all Rust
//! setup from the plan job and their crate jobs, mixed repos carry
//! the union in the plan while tofu groups stay pure.
use std::fs;

use velnor_actions_contract_workflow::StepKind;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::{
    FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID, WORKFLOW_PATH,
};

use super::impl_common::{TestResult, make_repo};

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

/// Fixture config with one tofu root.
fn tofu_config() -> String {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n"
        .to_owned()
}

/// Write one root with a non-empty fmt scope.
fn write_root(root: &std::path::Path) -> TestResult {
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    Ok(())
}

/// Step names of one finalized job.
fn names(job: &velnor_actions_contract_workflow::Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

/// Borrowed shell argv+env of one step.
type Shell<'a> = (
    &'a Vec<String>,
    &'a std::collections::BTreeMap<String, String>,
);

/// Shell argv+env of one named step.
fn shell_of<'a>(
    job: &'a velnor_actions_contract_workflow::Job,
    name: &str,
) -> Result<Shell<'a>, Box<dyn std::error::Error>> {
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

/// Pure-tofu repo: root crate removed, only the tofu root remains.
#[test]
fn pure_tofu_repo_drops_all_rust_setup() -> TestResult {
    let repo = make_repo(&tofu_config())?;
    let root = repo.path();
    write_root(root)?;
    fs::remove_file(root.join("Cargo.toml"))?;
    fs::remove_dir_all(root.join("src"))?;
    let prep = prepare(root)?;
    let jobs = finalized_jobs(&prep)?;
    let catalog = ToolCatalog::pinned();
    let plan = jobs.get("plan").ok_or("missing plan job")?;
    let steps = names(plan);
    assert!(
        !steps.contains(&"Prepare Rust components"),
        "pure-tofu plan has no components step: {steps:?}"
    );
    assert!(
        !steps.iter().any(|name| name.starts_with("Fetch Cargo")),
        "pure-tofu plan has no cargo fetch: {steps:?}"
    );
    let (run, env) = shell_of(plan, "Prepare pinned tools")?;
    assert!(
        !run.contains(&catalog.tool_spec(PinnedTool::Rust)),
        "pure-tofu plan installs no Rust: {run:?}"
    );
    assert!(
        run.contains(&catalog.tool_spec(PinnedTool::Opentofu)),
        "pure-tofu plan installs opentofu: {run:?}"
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(!env.contains_key(key), "plan prepare carries no {key}");
    }
    let tofu_jobs: Vec<(&String, &velnor_actions_contract_workflow::Job)> =
        jobs.iter().filter(|(id, _)| is_crate_job(id)).collect();
    assert_eq!(tofu_jobs.len(), 1, "one job for the tofu root");
    let job = tofu_jobs[0].1;
    let steps = names(job);
    assert!(
        !steps.contains(&"Prepare Rust components"),
        "tofu job has no components step: {steps:?}"
    );
    let (_, env) = shell_of(job, "Prepare pinned tools")?;
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(!env.contains_key(key), "tofu prepare carries no {key}");
    }
    let (_, env) = shell_of(job, "Validate")?;
    assert!(
        env.get("TF_DATA_DIR")
            .is_some_and(|dir| dir.contains("tofu-data")),
        "tofu obligation keeps its data dir: {env:?}"
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(
            !env.contains_key(key),
            "tofu obligation carries no {key}: {env:?}"
        );
    }
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let plan_window = job_window(yaml, "plan")?;
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:", "RUSTUP_TOOLCHAIN:"] {
        assert!(
            !plan_window.contains(key),
            "rendered plan job (freshness included) carries no {key}:\n{plan_window}"
        );
    }
    Ok(())
}

/// Mixed repo: the root crate stays, so the plan carries the union
/// while the tofu group stays pure and the rust group stays whole.
#[test]
fn mixed_repo_plan_carries_union_with_pure_tofu_group() -> TestResult {
    let repo = make_repo(&tofu_config())?;
    write_root(repo.path())?;
    let prep = prepare(repo.path())?;
    let jobs = finalized_jobs(&prep)?;
    let catalog = ToolCatalog::pinned();
    let plan = jobs.get("plan").ok_or("missing plan job")?;
    let (run, env) = shell_of(plan, "Prepare pinned tools")?;
    assert!(
        run.contains(&catalog.tool_spec(PinnedTool::Rust))
            && run.contains(&catalog.tool_spec(PinnedTool::Opentofu)),
        "mixed plan installs the union: {run:?}"
    );
    assert!(
        env.contains_key("RUSTUP_TOOLCHAIN"),
        "mixed plan keeps the triple"
    );
    assert!(
        names(plan).contains(&"Prepare Rust components"),
        "mixed plan keeps components"
    );
    let mut saw_rust = false;
    let mut saw_tofu = false;
    for (id, job) in &jobs {
        if !is_crate_job(id) {
            continue;
        }
        let (run, _) = shell_of(job, "Prepare pinned tools")?;
        let has_rust = run.contains(&catalog.tool_spec(PinnedTool::Rust));
        let has_tofu = run.contains(&catalog.tool_spec(PinnedTool::Opentofu));
        if has_rust && !has_tofu {
            saw_rust = true;
            assert!(
                names(job).contains(&"Prepare Rust components"),
                "{id} keeps components"
            );
        } else if has_tofu && !has_rust {
            saw_tofu = true;
            assert!(
                !names(job).contains(&"Prepare Rust components"),
                "{id} drops components"
            );
        } else {
            panic!("{id} must be rust-only or tofu-only: {run:?}");
        }
    }
    assert!(saw_rust && saw_tofu, "both groups render");
    Ok(())
}
