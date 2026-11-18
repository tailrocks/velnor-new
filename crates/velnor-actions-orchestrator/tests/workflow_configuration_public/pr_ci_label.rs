// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::source_qualification::WORKFLOW_PATH as SOURCE_QUALIFICATION_PATH;
use super::support::rendered_job_block;
use super::workflow_configuration_fixture;

const FULL_GATE: &str = "github.event_name != 'pull_request' || (github.event.pull_request.head.repo.fork == false && (contains(github.event.pull_request.labels.*.name, 'ci:run') || contains(github.event.pull_request.labels.*.name, 'ci:auto')))";
const ADMIT_GATE: &str = "github.event_name != 'pull_request' || (github.event.pull_request.draft == false && (github.event.action != 'labeled' || github.event.label.name == 'ci:run' || github.event.label.name == 'ci:auto'))";

fn config(pr_ci: Option<&str>) -> String {
    let pr_ci = pr_ci.map_or_else(String::new, |mode| format!("pr_ci = \"{mode}\"\n"));
    format!(
        "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"generator_release\"]\n{pr_ci}\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
    )
}

fn render(pr_ci: Option<&str>) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(&config(pr_ci))?;
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    Ok(tree
        .files
        .into_iter()
        .map(|file| (file.path, file.bytes))
        .collect())
}

fn text<'a>(tree: &'a BTreeMap<String, String>, path: &str) -> Result<&'a String, Box<dyn Error>> {
    tree.get(path)
        .ok_or_else(|| format!("{path} was not rendered").into())
}

#[test]
fn pr_ci_off_is_byte_identical_to_the_default() -> Result<(), Box<dyn Error>> {
    let default = render(None)?;
    let explicit = render(Some("off"))?;
    assert_eq!(default, explicit);
    let ci = text(&default, WORKFLOW_PATH)?;
    assert!(ci.contains("      - edited\n"));
    assert!(!ci.contains("labeled"));
    assert!(!ci.contains("pr-ci-claim"));
    assert!(!ci.contains("ci:run"));
    let source = text(&default, SOURCE_QUALIFICATION_PATH)?;
    assert!(source.contains("group: source-qualification-${{ github.ref }}\n"));
    assert!(!source.contains("ci:run"));
    Ok(())
}

#[test]
fn pr_ci_label_rewrites_the_ci_workflow() -> Result<(), Box<dyn Error>> {
    let tree = render(Some("label"))?;
    let ci = text(&tree, WORKFLOW_PATH)?;
    assert!(ci.contains(
        "pull_request:\n    types:\n      - opened\n      - synchronize\n      - reopened\n      - ready_for_review\n      - labeled\n  push:"
    ));
    assert!(!ci.contains("      - edited\n"));
    assert!(ci.contains("  merge_group:\n"));
    assert!(ci.contains(
        "  group: velnor-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}${{ github.event.action == 'labeled'"
    ));
    assert!(ci.contains("  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n"));

    let plan = rendered_job_block(ci, "plan")?;
    assert!(plan.contains(&format!("    if: ${{{{ ({ADMIT_GATE}) }}}}\n")));
    assert!(!plan.contains(FULL_GATE));

    let rust = rendered_job_block(ci, "rust-velnor-actions-cli")?;
    assert!(rust.contains(&format!("({ADMIT_GATE}) && ({FULL_GATE})")));

    let required = rendered_job_block(ci, "required")?;
    assert!(required.contains(&format!(
        "    if: ${{{{ (!cancelled()) && ({ADMIT_GATE}) && ({FULL_GATE}) }}}}\n"
    )));

    let gate = rendered_job_block(ci, "required-gate")?;
    assert!(gate.contains("    needs:\n      - required\n      - plan\n"));
    assert!(gate.contains(&format!("    if: ${{{{ always() && ({ADMIT_GATE}) }}}}\n")));
    assert!(gate.contains("needs.required.result != 'success'"));
    assert!(gate.contains(&format!("!({FULL_GATE})")));

    let claim = rendered_job_block(ci, "pr-ci-claim")?;
    assert!(claim.contains("    permissions:\n      pull-requests: write\n"));
    assert!(claim.contains("github.event.pull_request.head.repo.fork == false"));
    assert!(claim.contains("contains(github.event.pull_request.labels.*.name, 'ci:run')"));
    assert!(claim.contains("-X DELETE"));
    assert!(!claim.contains("needs:"));
    assert_eq!(ci.matches("pull-requests: write").count(), 1);
    Ok(())
}

#[test]
fn pr_ci_label_gate_tolerates_skipped_quick_lanes_but_not_failed_ones() -> Result<(), Box<dyn Error>>
{
    let tree = render(Some("label"))?;
    let ci = text(&tree, WORKFLOW_PATH)?;
    let gate = rendered_job_block(ci, "required-gate")?;
    let quick: Vec<&str> = gate
        .split("    needs:\n")
        .nth(1)
        .and_then(|rest| rest.split("    if:").next())
        .ok_or("gate needs are missing")?
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .filter(|id| *id != "required")
        .collect();
    assert!(quick.contains(&"plan"), "plan is a quick need: {quick:?}");
    let failed = quick
        .iter()
        .map(|id| format!("needs.{id}.result == 'failure' || needs.{id}.result == 'cancelled'"))
        .collect::<Vec<_>>()
        .join(" || ");
    // pull_request without a request label: a skipped quick lane passes, a
    // failed or cancelled one fails, and `required` is not consulted.
    // merge_group and push: the full strict verdict alone decides.
    let verdict = format!(
        "((({FULL_GATE}) && needs.required.result != 'success') || (!({FULL_GATE}) && ({failed})))"
    );
    assert!(
        gate.contains(&format!("if: ${{{{ always() && {verdict} }}}}")),
        "{gate}"
    );
    for id in &quick {
        assert!(
            !gate.contains(&format!("needs.{id}.result != 'success'")),
            "{id} must not fail the gate when skipped"
        );
    }
    let on = ci.split("\njobs:").next().unwrap_or_default();
    assert!(on.contains("  merge_group:") && on.contains("  push:"));
    Ok(())
}

#[test]
fn pr_ci_label_gates_source_qualification() -> Result<(), Box<dyn Error>> {
    let tree = render(Some("label"))?;
    let source = text(&tree, SOURCE_QUALIFICATION_PATH)?;
    assert!(source.contains(
        "pull_request:\n    types:\n      - opened\n      - synchronize\n      - reopened\n      - ready_for_review\n      - labeled\n"
    ));
    assert!(source.contains(
        "group: source-qualification-${{ github.event.pull_request.number || github.ref }}${{ github.event.action == 'labeled'"
    ));
    assert!(source.contains("cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n"));
    assert!(!source.contains("pr-ci-claim"));
    for job in [
        "plan",
        "selection",
        "ordinary-harness",
        "keep-harness",
        "generation-checks",
        "source-qualification",
    ] {
        let block = rendered_job_block(source, job)?;
        assert!(
            block.contains(&format!("({ADMIT_GATE}) && ({FULL_GATE})")),
            "{job} is not gated"
        );
    }
    Ok(())
}

#[test]
fn pr_ci_label_runs_every_pull_request_workflow_on_merge_group() -> Result<(), Box<dyn Error>> {
    let tree = render(Some("label"))?;
    let source = text(&tree, SOURCE_QUALIFICATION_PATH)?;
    assert!(source.contains("  merge_group:\n"));
    let mut checked = 0;
    for (path, body) in &tree {
        if !path.starts_with(".github/workflows/") {
            continue;
        }
        let on = body.split("\njobs:").next().unwrap_or_default();
        if on.contains("  pull_request:\n") {
            assert!(on.contains("  merge_group:"), "{path} lacks merge_group");
            checked += 1;
        }
    }
    assert!(checked >= 2, "expected at least two pull_request workflows");
    Ok(())
}

#[test]
fn pr_ci_rejects_unknown_modes() -> Result<(), Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(&config(Some("always")))?;
    assert!(prepare(repo.path()).is_err());
    Ok(())
}
