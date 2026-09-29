//! W1 emission-wiring regressions: orchestrator halves of cross-crate TODOs.

use std::fs;

use tempfile::TempDir;
use velnor_actions_actionlint::tools::{ActionlintToolchain, ShellcheckToolchain};
use velnor_actions_actionlint::{
    ActionlintCapabilities, PinnedActionRef, StepSyntax, checkout_inputs_schema,
    render_actionlint_yaml, validate_action_inputs,
};
use velnor_actions_actionlint::{ActionlintConfigInput, RUNNER_LABEL_BRIDGE};
use velnor_actions_mise::{CandidateBuild, PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{DEFAULT_RUNNER_LABEL, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::{ACTIONLINT_PATH, WORKFLOW_PATH};

use super::impl_common::{TestResult, config_with_branch, make_repo};

/// Staged workflow + actionlint bytes for one config.
fn preview_both(config: &str) -> Result<(TempDir, String, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow")?
        .to_owned();
    let actionlint = tree
        .get(ACTIONLINT_PATH)
        .ok_or("missing actionlint")?
        .to_owned();
    Ok((repo, workflow, actionlint))
}

/// Velnor-policy fixture: canonical origin, no lock (pre-seed shape).
fn make_velnor_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let git_config = repo.path().join(".git/config");
    let mut text = fs::read_to_string(&git_config)?;
    text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
    fs::write(&git_config, text)?;
    Ok(repo)
}

/// Skip only when an ambient non-canonical identity would fail the fixture.
fn ambient_identity_blocks() -> bool {
    std::env::var("GITHUB_REPOSITORY").is_ok_and(|hint| hint != "tailrocks/velnor-new")
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
    match tail.find(end) {
        Some(at) => Ok(&tail[..at]),
        None => Ok(tail),
    }
}

const VELNOR_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n";

#[test]
fn w1_checkout_uses_pinned_action_ref() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    for want in [
        "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "persist-credentials: \"false\"",
    ] {
        assert!(yaml.contains(want), "missing {want}:\n{yaml}");
    }
    let uses = PinnedActionRef::checkout().uses_value();
    assert_eq!(
        uses,
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
    );
    let schema = checkout_inputs_schema();
    let good =
        std::collections::BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    assert!(validate_action_inputs(&schema, &good).is_ok());
    assert!(validate_action_inputs(&schema, &std::collections::BTreeMap::new()).is_err());
    Ok(())
}

#[test]
fn w1_runs_on_hosted_labels_only() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let (_repo, consumer, _alint) = preview_both(config_with_branch())?;
    let velnor = make_velnor_repo(VELNOR_CONFIG)?;
    let prep = prepare(velnor.path())?;
    let tree = render_staged_tree(&prep)?;
    let velnor_yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    for (name, yaml) in [("consumer", consumer.as_str()), ("velnor", velnor_yaml)] {
        let labels: Vec<&str> = yaml
            .lines()
            .filter_map(|line| line.trim().strip_prefix("runs-on:"))
            .map(str::trim)
            .collect();
        assert!(labels.len() >= 4, "{name} jobs: {labels:?}");
        for label in &labels {
            assert!(label.starts_with("ubuntu-"), "{name} label {label}");
        }
        assert!(!yaml.contains("self-hosted"), "{name} must not self-host");
    }
    Ok(())
}

#[test]
fn w1_runner_label_matches_actionlint_bridge() {
    assert_eq!(DEFAULT_RUNNER_LABEL, RUNNER_LABEL_BRIDGE);
}

#[test]
fn w1_lint_run_embeds_crate_tool_specs() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let lint = window(&yaml, "  velnor-workflow-lint:", "  velnor-zzz:")?;
    for spec in [
        ActionlintToolchain::pinned().mise_tool_spec(),
        ShellcheckToolchain::pinned().mise_tool_spec(),
    ] {
        assert!(lint.contains(&spec), "lint misses {spec}:\n{lint}");
    }
    assert!(lint.contains("actionlint -color"), "lint argv:\n{lint}");
    Ok(())
}

#[test]
fn w1_catalog_matches_actionlint_crate_pins() {
    let catalog = ToolCatalog::pinned();
    assert_eq!(
        catalog.tool_spec(PinnedTool::Actionlint),
        ActionlintToolchain::pinned().mise_tool_spec()
    );
    assert_eq!(
        catalog.tool_spec(PinnedTool::Shellcheck),
        ShellcheckToolchain::pinned().mise_tool_spec()
    );
    assert_eq!(
        velnor_actions_mise::catalog::ACTIONLINT_VERSION,
        velnor_actions_actionlint::capabilities::ACTIONLINT_VERSION
    );
}

#[test]
fn w1_native_parallelism_unqualified() {
    let caps = ActionlintCapabilities::for_pinned();
    assert!(caps.check_step_syntax(StepSyntax::JobMatrix).is_ok());
    let err = caps
        .check_step_syntax(StepSyntax::NativeParallelism)
        .expect_err("native gated");
    assert!(err.to_string().contains("native_parallelism"), "{err}");
}

#[test]
fn w1_task_job_prepares_pinned_tools() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let task = window(&yaml, "  velnor-task:", "  velnor-workflow-lint:")?;
    let checkout = task.find("- name: Checkout").ok_or("task checkout")?;
    let prepare = task
        .find("- name: Prepare pinned tools")
        .ok_or("task prepare")?;
    let run = task.find("- name: Run task").ok_or("task run")?;
    assert!(checkout < prepare && prepare < run, "order:\n{task}");
    let catalog = ToolCatalog::pinned();
    assert!(
        task.contains(&format!("install {}", catalog.tool_spec(PinnedTool::Rust))),
        "install:\n{task}"
    );
    assert!(
        !task.contains("mr-boxington"),
        "cargo leg MBX-free:\n{task}"
    );
    Ok(())
}

#[test]
fn w1_task_prepare_adds_mbx_driver() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    with_mbx(&repo)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let task = window(yaml, "  velnor-task:", "  velnor-workflow-lint:")?;
    let catalog = ToolCatalog::pinned();
    assert!(
        task.contains(&catalog.tool_spec(PinnedTool::MrBoxington)),
        "mbx spec:\n{task}"
    );
    assert_eq!(
        task.matches("Restore MBX objects").count(),
        1,
        "one objects step:\n{task}"
    );
    assert!(
        task.contains("mode: objects") || task.contains("mode: \"objects\""),
        "{task}"
    );
    Ok(())
}

#[test]
fn w1_plan_prepare_index_and_contract_order() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let plan = window(&yaml, "  velnor-plan:", "  velnor-task:")?;
    let names: Vec<&str> = plan
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- name: "))
        .collect();
    let at = |name: &'static str| names.iter().position(|step| *step == name).ok_or(name);
    let (checkout, prepare, write, format, plan_step) = (
        at("Checkout")?,
        at("Prepare pinned tools")?,
        at("Write request")?,
        at("Format")?,
        at("Plan")?,
    );
    assert!(checkout < prepare && prepare < write, "{names:?}");
    assert!(write < format && format < plan_step, "{names:?}");
    Ok(())
}

#[test]
fn w1_candidate_build_matches_mise_constructor() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let velnor = make_velnor_repo(VELNOR_CONFIG)?;
    let prep = prepare(velnor.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let catalog = ToolCatalog::pinned();
    let want = CandidateBuild::new()?
        .argv(&catalog)
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non-utf8"))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    assert!(yaml.contains(&want), "preseed build misses {want}:\n{yaml}");
    Ok(())
}

#[test]
fn w1_policy_carries_zizmor_after_machete() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let velnor = make_velnor_repo(VELNOR_CONFIG)?;
    let tree = render_staged_tree(&prepare(velnor.path())?)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let policy = window(yaml, "  velnor-policy:", "  velnor-task:")?;
    let deny = policy.find("Run cargo-deny").ok_or("deny")?;
    let machete = policy.find("Run cargo-machete").ok_or("machete")?;
    let zizmor = policy.find("Run zizmor").ok_or("zizmor")?;
    assert!(deny < machete && machete < zizmor, "order:\n{policy}");
    let catalog = ToolCatalog::pinned();
    assert!(
        policy.contains(&catalog.tool_spec(PinnedTool::Zizmor)),
        "{policy}"
    );
    assert!(policy.contains("--no-online-audits"), "{policy}");
    Ok(())
}

#[test]
fn w1_plan_format_runs_fmt_check() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let plan = window(&yaml, "  velnor-plan:", "  velnor-task:")?;
    assert!(plan.contains("- name: Format"), "format step:\n{plan}");
    assert!(
        plan.contains("mise ") && plan.contains("cargo fmt --check"),
        "{plan}"
    );
    let ignored =
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
    let (_repo, yaml, _alint) = preview_both(ignored)?;
    assert!(
        !yaml.contains("- name: Format"),
        "no fmt without rust:\n{yaml}"
    );
    Ok(())
}

#[test]
fn w1_actionlint_vars_emit_exact_v1_set() -> TestResult {
    let (_repo, _yaml, actionlint) = preview_both(config_with_branch())?;
    assert!(
        actionlint.contains("config-variables: []"),
        "v1 empty set:\n{actionlint}"
    );
    assert!(
        actionlint.contains("self-hosted-runner:"),
        "bridge:\n{actionlint}"
    );
    let input = ActionlintConfigInput::new("0.1.0")
        .with_workflow_path(WORKFLOW_PATH)
        .with_config_variables(["API_BASE"]);
    let yaml = render_actionlint_yaml(&input)?.yaml;
    assert_eq!(
        yaml,
        "# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate.\n\nconfig-variables:\n  - API_BASE\n\nself-hosted-runner:\n  labels:\n    - ubuntu-26.04\n"
    );
    Ok(())
}

#[test]
fn w1_pr_workflows_carry_no_msrv() -> TestResult {
    if ambient_identity_blocks() {
        return Ok(());
    }
    let (_repo, consumer, _alint) = preview_both(config_with_branch())?;
    let velnor = make_velnor_repo(VELNOR_CONFIG)?;
    let tree = render_staged_tree(&prepare(velnor.path())?)?;
    let velnor_yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    for (name, yaml) in [("consumer", consumer.as_str()), ("velnor", velnor_yaml)] {
        assert!(!yaml.to_lowercase().contains("msrv"), "{name} leaks msrv");
    }
    Ok(())
}

#[test]
fn w1_task_cache_v1_emits_no_cache_steps() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let task = window(&yaml, "  velnor-task:", "  velnor-workflow-lint:")?;
    assert!(
        !task.contains("- name: Restore cache"),
        "v1 no restore:\n{task}"
    );
    assert!(!task.contains("- name: Save cache"), "v1 no save:\n{task}");
    Ok(())
}
