// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, fs};

#[path = "both_shared_producers/binding.rs"]
mod binding;
#[path = "both_shared_producers/preview.rs"]
mod preview;

use self::binding::{assert_dispositions, assert_hosted_wire};
use super::scoped_workspace::commit;
use velnor_actions_contract::{
    ExecutionMode, StepKind, WorkflowPolicy, expand_workflow_with_lane_plan, named_check_lanes,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo_for;
use super::support::rendered_job_block;
use super::workflow_configuration_fixture::write_root_cargo_lock;

#[test]
fn stable_both_keeps_shared_producers_and_splits_named_check() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_for(true, WorkflowPolicy::VelnorRepositoryV1)?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n",
    )?;
    fs::remove_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("crates/maintenance/src"))?;
    fs::write(
        root.join("crates/maintenance/Cargo.toml"),
        "[package]\nname = \"maintenance\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(
        root.join("crates/maintenance/src/lib.rs"),
        "pub fn maintenance() {}\n",
    )?;
    write_root_cargo_lock(root, &["maintenance"])?;
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?.replace("schema = 1", "schema = 2")
        + "\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n";
    fs::write(
        &config_path,
        format!(
            "{config}\n[execution]\nmode = \"both\"\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n\n[[checks]]\nid = \"maintenance-helpers\"\ntask = \"fixture-check\"\ninputs = [\"crates/maintenance/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
        ),
    )?;
    let mise = root.join("mise.toml");
    let content = fs::read_to_string(&mise).unwrap_or_default();
    fs::write(
        &mise,
        format!("{content}\n[tasks.fixture-check]\nrun = \"true\"\n"),
    )?;
    let prepared = prepare(root)?;
    let lanes = named_check_lanes(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let ids: BTreeSet<_> = lanes["check-maintenance-helpers"]
        .iter()
        .map(|lane| lane.job_id.as_str())
        .collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "check-maintenance-helpers__hosted",
            "check-maintenance-helpers__local"
        ])
    );
    let hosted_tree = render_staged_tree_with(&prepared, Some(ExecutionMode::Hosted))?;
    let hosted_workflow = hosted_tree
        .get(WORKFLOW_PATH)
        .ok_or("missing hosted workflow")?;
    rendered_job_block(hosted_workflow, "check-maintenance-helpers")?;
    rendered_job_block(hosted_workflow, "ledger-cli")?;
    let tree = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    for id in &ids {
        rendered_job_block(workflow, id)?;
    }
    let hosted = rendered_job_block(workflow, "check-maintenance-helpers__hosted")?;
    let local = rendered_job_block(workflow, "check-maintenance-helpers__local")?;
    assert!(hosted.contains("VELNOR_CHECK_JOB_ID: check-maintenance-helpers__hosted"));
    assert!(hosted.contains("VELNOR_CHECK_LANE_VARIANT: hosted"));
    assert!(local.contains("VELNOR_CHECK_JOB_ID: check-maintenance-helpers__local"));
    assert!(local.contains("VELNOR_CHECK_LANE_VARIANT: scale_set"));
    assert!(local.contains(velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION));
    let producers: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.strip_prefix("  ")?.strip_suffix(':'))
        .filter(|id| {
            id.starts_with("tools-producer-")
                || id.starts_with("providers-producer-")
                || *id == "sources-producer"
        })
        .collect();
    assert!(
        producers.iter().any(|id| id.starts_with("tools-producer-")),
        "{producers:?}"
    );
    assert!(producers.contains(&"sources-producer"), "{producers:?}");
    for id in producers {
        let block = rendered_job_block(workflow, id)?;
        assert!(block.contains("ubuntu-26.04"), "{id}: {block}");
        assert!(!workflow.contains(&format!("{id}__hosted:")));
        assert!(!workflow.contains(&format!("{id}__local:")));
    }
    let ledger = rendered_job_block(workflow, "ledger-cli")?;
    assert!(ledger.contains("Build ledger CLI"));
    assert!(ledger.contains("ubuntu-26.04"));
    assert!(!workflow.contains("ledger-cli__hosted:"));
    assert!(!workflow.contains("ledger-cli__local:"));
    let required = rendered_job_block(workflow, "required")?;
    assert!(!required.contains("ledger-cli"));
    let job = prepared
        .workflow
        .ir
        .jobs
        .get("ledger-cli")
        .ok_or("missing ledger")?;
    let upload = job
        .steps
        .iter()
        .find_map(|step| match &step.kind {
            StepKind::Action { uses, with, .. } if uses.starts_with("actions/upload-artifact@") => {
                with.get("name")
            }
            _ => None,
        })
        .ok_or("missing ledger upload")?;
    assert_eq!(Some(upload), job.outputs.get("artifact_name"));
    preview::assert_preview_render(root)?;
    let without_groups =
        fs::read_to_string(&config_path)?.replace("\n[stacks.tofu]\nroots = [\"infra\"]\n", "\n");
    fs::write(&config_path, without_groups)?;
    let prepared = prepare(root)?;
    let lanes = named_check_lanes(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let expanded = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let required_ids = &expanded.ir.jobs["required"].needs;
    let inventory = expanded
        .lane_plan
        .as_ref()
        .ok_or("missing lane plan")?
        .inventory();
    let hosted_lanes = named_check_lanes(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Hosted),
    )?;
    let scoped = fs::read_to_string(&config_path)?.replace(
        "task = \"fixture-check\"",
        "task = \"fixture-check\"\nselection = \"declared_inputs\"",
    );
    fs::write(&config_path, scoped)?;
    let stable = prepare(root)?;
    let output = tempfile::tempdir()?;
    let destination = output.path().join("rejected-stable");
    let error = velnor_actions_orchestrator::generate(
        &stable,
        &velnor_actions_orchestrator::GenerateOptions {
            output_dir: Some(destination.clone()),
        },
    )
    .err()
    .ok_or("stable scoped generation was accepted")?;
    assert!(
        error
            .to_string()
            .contains("declared_inputs_requires_approved_preview_controller")
    );
    assert!(!destination.exists());
    preview::assert_preview_render(root)?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("docs/guide.md"), "baseline\n")?;
    let base = commit(root, "configure scoped check")?;
    fs::write(
        root.join("crates/maintenance/src/lib.rs"),
        "pub fn changed() {}\n",
    )?;
    let affected = commit(root, "affect check")?;
    assert_dispositions(
        root,
        &base,
        &affected,
        &lanes,
        &inventory,
        required_ids,
        "run",
    )?;
    assert_hosted_wire(root, &base, &affected, &hosted_lanes)?;
    fs::write(root.join("docs/guide.md"), "changed\n")?;
    let unrelated = commit(root, "change unrelated crate")?;
    assert_dispositions(
        root,
        &affected,
        &unrelated,
        &lanes,
        &inventory,
        required_ids,
        "not_affected",
    )?;
    Ok(())
}
