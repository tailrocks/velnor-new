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

use super::impl_common::{
    TestResult, config_with_branch, make_repo, make_virtual_repo, without_ambient_identity,
};

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
    without_ambient_identity("w1_runs_on_hosted_labels_only", || {
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
    })
}

#[test]
fn w1_runner_label_matches_actionlint_bridge() {
    assert_eq!(DEFAULT_RUNNER_LABEL, RUNNER_LABEL_BRIDGE);
}

#[test]
fn w1_actionlint_bridge_uses_configured_runner_label() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nrunner_label = \"ubuntu-24.04\"\n";
    let (_repo, yaml, actionlint) = preview_both(config)?;
    assert!(
        yaml.contains("runs-on: ubuntu-24.04"),
        "workflow must use the configured label"
    );
    assert!(
        actionlint.contains("    - ubuntu-24.04\n"),
        "bridge must emit the configured label, never a hardcoded distro:\n{actionlint}"
    );
    assert!(
        !actionlint.contains("ubuntu-26.04"),
        "bridge must not leak the default distro:\n{actionlint}"
    );
    Ok(())
}

#[test]
fn w1_lint_run_embeds_crate_tool_specs() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let lint = window(&yaml, "  actionlint:", "  velnor-zzz:")?;
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
fn w1_plan_prepare_index_and_contract_order() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let plan = window(&yaml, "  plan:", "  required:")?;
    let names: Vec<&str> = plan
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- name: "))
        .collect();
    let at = |name: &'static str| names.iter().position(|step| *step == name).ok_or(name);
    let (checkout, prepare, write, plan_step) = (
        at("Checkout")?,
        at("Prepare pinned tools")?,
        at("Write request")?,
        at("Plan")?,
    );
    assert!(checkout < prepare && prepare < write, "{names:?}");
    assert!(write < plan_step, "{names:?}");
    assert!(
        !names.contains(&"Format"),
        "no workspace scope without rustfmt config: {names:?}"
    );
    Ok(())
}

#[test]
fn w1_candidate_build_matches_mise_constructor() -> TestResult {
    without_ambient_identity("w1_candidate_build_matches_mise_constructor", || {
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
    })
}

#[test]
fn w1_validators_carry_deny_machete_zizmor_in_order() -> TestResult {
    without_ambient_identity("w1_validators_carry_deny_machete_zizmor_in_order", || {
        let velnor = make_velnor_repo(VELNOR_CONFIG)?;
        let tree = render_staged_tree(&prepare(velnor.path())?)?;
        let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
        assert!(!yaml.contains("  policy:"), "no umbrella:\n{yaml}");
        let deny_at = yaml.find("  cargo-deny:").ok_or("deny job")?;
        let machete_at = yaml.find("  cargo-machete:").ok_or("machete job")?;
        let zizmor_at = yaml.find("  zizmor:").ok_or("zizmor job")?;
        assert!(
            deny_at < machete_at && machete_at < zizmor_at,
            "validator order:\n{yaml}"
        );
        let deny = window(yaml, "  cargo-deny:", "  cargo-machete:")?;
        let machete = window(yaml, "  cargo-machete:", "  plan:")?;
        let zizmor = window(yaml, "  zizmor:", "\nzzz-no-such-job:")?;
        assert!(deny.contains("Run cargo-deny"), "deny step:\n{deny}");
        assert!(
            machete.contains("Run cargo-machete"),
            "machete step:\n{machete}"
        );
        assert!(zizmor.contains("Run zizmor"), "zizmor step:\n{zizmor}");
        let catalog = ToolCatalog::pinned();
        assert!(
            zizmor.contains(&catalog.tool_spec(PinnedTool::Zizmor)),
            "{zizmor}"
        );
        assert!(zizmor.contains("--no-online-audits"), "{zizmor}");
        assert!(
            zizmor.contains("zizmor --no-online-audits --config .zizmor.yml .github/workflows"),
            "zizmor input+config:\n{zizmor}"
        );
        Ok(())
    })
}

#[test]
fn w1_plan_format_runs_fmt_check() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let plan = window(&yaml, "  plan:", "  required:")?;
    assert!(
        !plan.contains("- name: Format"),
        "no workspace scope without rustfmt config:\n{plan}"
    );
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("rustfmt.toml"), "[rustfmt]\n")?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    // R28: per-package Fmt groups own every file, so the plan job carries no
    // overlapping `fmt --all` scope; the crate job keeps its Format step.
    let plan = window(yaml, "  plan:", "  required:")?;
    assert!(
        !plan.contains("- name: Format"),
        "root package owns its fmt scope:\n{plan}"
    );
    let job = window(yaml, "  rust-demo:", "  required:")?;
    assert!(job.contains("- name: Format"), "per-package scope:\n{job}");
    {
        // Virtual workspace with members: per-package groups still own every
        // file, so no workspace `fmt --all` group exists and the plan job
        // owns no Format scope either (R28/P05-5 no-overlap). The duplicate
        // task id 6e2d4d4 fixed cannot recur: one group, one id.
        let virtual_repo = make_virtual_repo(config_with_branch())?;
        fs::write(virtual_repo.path().join("rustfmt.toml"), "[rustfmt]\n")?;
        let prep = prepare(virtual_repo.path())?;
        let tree = render_staged_tree(&prep)?;
        let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
        // Validators sort before plan, so the plan window ends at required.
        let plan = window(yaml, "  plan:", "  required:")?;
        assert!(
            !plan.contains("- name: Format"),
            "no overlapping plan scope in virtual workspaces:\n{plan}"
        );
        assert!(
            !yaml.contains("fmt --all --check"),
            "no workspace-wide format in virtual workspaces:\n{yaml}"
        );
    }
    assert!(
        !plan.contains("- name: Format"),
        "no overlapping plan scope:\n{plan}"
    );
    assert!(
        !yaml.contains("fmt --all --check"),
        "no workspace-wide format:\n{yaml}"
    );
    let job = window(yaml, "  rust-demo:", "  required:")?;
    assert!(job.contains("- name: Format"), "per-package scope:\n{job}");
    let format_at = job.find("- name: Format").ok_or("format step")?;
    let tail = &job[format_at..];
    let block = &tail[..tail.len().min(900)];
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:"] {
        assert!(block.contains(key), "format env misses {key}:\n{block}");
    }
    assert!(
        job.contains("RUSTUP_TOOLCHAIN: 1.98.1"),
        "job env misses RUSTUP_TOOLCHAIN:\n{job}"
    );
    for key in ["MISE_AUTO_INSTALL:", "MISE_EXEC_AUTO_INSTALL:"] {
        assert!(job.contains(key), "job env misses {key}:\n{job}");
    }
    // Regression: exactly one Format step per crate job, none in plan.
    let formats = yaml.matches("- name: Format").count();
    let crates = prep
        .workflow
        .ir
        .jobs
        .keys()
        .filter(|id| id.starts_with("rust-"))
        .count();
    assert!(crates >= 1, "fixture needs a crate job");
    assert_eq!(formats, crates, "one Format per crate, none elsewhere");
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
        .with_runner_label(RUNNER_LABEL_BRIDGE)
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
    without_ambient_identity("w1_pr_workflows_carry_no_msrv", || {
        let (_repo, consumer, _alint) = preview_both(config_with_branch())?;
        let velnor = make_velnor_repo(VELNOR_CONFIG)?;
        let tree = render_staged_tree(&prepare(velnor.path())?)?;
        let velnor_yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
        for (name, yaml) in [("consumer", consumer.as_str()), ("velnor", velnor_yaml)] {
            assert!(!yaml.to_lowercase().contains("msrv"), "{name} leaks msrv");
        }
        Ok(())
    })
}
