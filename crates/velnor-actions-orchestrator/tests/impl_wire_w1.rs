//! W1 emission-wiring regressions: orchestrator halves of cross-crate TODOs.

use std::fs;

use tempfile::TempDir;
use velnor_actions_actionlint::tools::{JactionlintToolchain, ShellcheckToolchain};
use velnor_actions_actionlint::{
    JactionlintCapabilities, JactionlintConfigInput, PinnedActionRef, StepSyntax,
    checkout_inputs_schema, render_jactionlint_yaml, validate_action_inputs,
};
use velnor_actions_contract::workflow::StepKind;
use velnor_actions_mise::{CandidateBuild, PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::{JACTIONLINT_PATH, WORKFLOW_PATH};

#[path = "impl_wire_w1_format_helpers.rs"]
mod format_helpers;

use super::impl_common::{
    TestResult, config_with_branch, make_repo, make_virtual_repo, without_ambient_identity,
};

/// Staged workflow + jactionlint config bytes for one config.
fn preview_both(config: &str) -> Result<(TempDir, String, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow")?
        .to_owned();
    let lint_config = tree
        .get(JACTIONLINT_PATH)
        .ok_or("missing jactionlint config")?
        .to_owned();
    Ok((repo, workflow, lint_config))
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
fn w1_lint_config_declares_no_github_hosted_label() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nrunner_label = \"ubuntu-24.04\"\n";
    let (_repo, yaml, lint_config) = preview_both(config)?;
    assert!(
        yaml.contains("runs-on: ubuntu-24.04"),
        "workflow must use the configured label"
    );
    // jactionlint knows every hosted label natively; declaring one as
    // self-hosted would make it treat GitHub's runners as self-hosted.
    assert!(
        !lint_config.contains("self-hosted-runner"),
        "no bridge label:\n{lint_config}"
    );
    assert!(
        lint_config.contains("\nprofile: correctness\n"),
        "{lint_config}"
    );
    assert!(
        lint_config.contains("\ngenerated-files: report\n"),
        "{lint_config}"
    );
    Ok(())
}

#[test]
fn w1_lint_run_embeds_crate_tool_specs_and_the_hk_file_set() -> TestResult {
    let (_repo, yaml, _lint_config) = preview_both(config_with_branch())?;
    let lint = window(&yaml, "  jactionlint:", "  plan:")?;
    for spec in [
        JactionlintToolchain::pinned().mise_tool_spec(),
        ShellcheckToolchain::pinned().mise_tool_spec(),
    ] {
        assert!(lint.contains(&spec), "lint misses {spec}:\n{lint}");
    }
    assert!(lint.contains("name: Verify jactionlint"), "{lint}");
    for find in [
        r#"find .github/workflows -maxdepth 1 -type f -name \"*.y*ml\";"#,
        r#"find .github/actions -type f -name \"action.y*ml\";"#,
        r#"find .github -maxdepth 1 -type f -name \"dependabot.y*ml\";"#,
    ] {
        assert!(lint.contains(find), "lint argv misses {find}:\n{lint}");
    }
    assert!(
        lint.contains("exec mise --no-config --no-env --no-hooks exec jactionlint@2.3.0 shellcheck@0.11.0 -- jactionlint --config-file .github/jactionlint.yaml --format github --strict-exit --no-online --shellcheck=shellcheck --pyflakes= --no-color \\\"${files[@]}\\\"'"),
        "lint argv:\n{lint}"
    );
    assert!(
        lint.contains("bash -c 'unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "{lint}"
    );
    assert!(!lint.contains("JACTIONLINT_NO_HINTS"), "{lint}");
    Ok(())
}

#[test]
fn w1_catalog_matches_adapter_crate_pins() {
    let catalog = ToolCatalog::pinned();
    assert_eq!(
        catalog.tool_spec(PinnedTool::Jactionlint),
        JactionlintToolchain::pinned().mise_tool_spec()
    );
    assert_eq!(
        catalog.tool_spec(PinnedTool::Shellcheck),
        ShellcheckToolchain::pinned().mise_tool_spec()
    );
    assert_eq!(
        velnor_actions_mise::catalog::JACTIONLINT_VERSION,
        velnor_actions_actionlint::capabilities::JACTIONLINT_VERSION
    );
}

#[test]
fn w1_native_parallelism_and_cache_mode_need_no_bridge() {
    let caps = JactionlintCapabilities::for_pinned();
    for syntax in [
        StepSyntax::JobMatrix,
        StepSyntax::NativeParallelism,
        StepSyntax::CacheMode,
    ] {
        assert!(caps.check_step_syntax(syntax).is_ok(), "{syntax:?}");
    }
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
        let mut candidate = CandidateBuild::new()?
            .argv(&catalog)
            .into_iter()
            .map(|arg| arg.into_string().map_err(|_| "non-utf8"))
            .collect::<Result<Vec<_>, _>>()?;
        let mbx = candidate
            .iter()
            .position(|arg| arg == "mbx")
            .ok_or("candidate vector has no MBX command")?;
        candidate[mbx] = "\\\"$mbx_bin\\\"".to_owned();
        let want = candidate.join(" ");
        let build = window(
            yaml,
            "name: Build helper (pre-seed trust-on-review)",
            "name: Verify MBX compile (pre-seed trust-on-review)",
        )?;
        assert!(
            build.contains("command -v mbx"),
            "pre-seed resolves action-owned MBX before Mise: {build}"
        );
        assert!(
            build.contains(&want),
            "preseed build must bind the shared candidate vector to the resolved MBX binary; expected {want}:\n{build}"
        );
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
    format_helpers::assert_no_format_without_rustfmt_config()?;
    format_helpers::assert_configured_format_is_per_crate()?;
    format_helpers::assert_virtual_workspace_has_no_overlap()?;
    format_helpers::assert_ignored_rust_has_no_format()?;
    Ok(())
}

#[test]
fn w1_lint_config_vars_emit_exact_v1_set() -> TestResult {
    let (_repo, _yaml, lint_config) = preview_both(config_with_branch())?;
    assert!(
        lint_config.contains("config-variables: []"),
        "v1 empty set:\n{lint_config}"
    );
    let input = JactionlintConfigInput::new("0.1.0").with_config_variables(["API_BASE"]);
    let yaml = render_jactionlint_yaml(&input)?;
    assert_eq!(
        yaml,
        "# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate.\n\nprofile: correctness\ngenerated-files: report\nconfig-variables:\n  - API_BASE\n"
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
