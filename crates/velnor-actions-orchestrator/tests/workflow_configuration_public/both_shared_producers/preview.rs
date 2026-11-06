// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::{error::Error, fmt::Write as _, fs};
use velnor_actions_contract::ExecutionMode;
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RECEIPT: &str = "2d133dc7766edd85d88b9ec8d132f075ebad232849500277ef3e6f5f93beefce";

pub(super) fn assert_preview_render(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let path = root.join(".velnor/generator.lock");
    let original = fs::read_to_string(&path)?;
    let mut lock = original.clone();
    write!(
        &mut lock,
        "\n[planner-preview]\nschema = 2\nrepository = \"tailrocks/velnor-new\"\nversion = \"0.1.75\"\nsource_sha = \"{SOURCE}\"\ntag = \"sdk-preview-{SOURCE}\"\nrelease_id = 10\n"
    )?;
    for (role, workflow, id) in [
        (
            "source",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        (
            "draft",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        (
            "signer",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        ("ci", ".github/workflows/ci.yml", 12),
        (
            "qualification",
            ".github/workflows/source-qualification.yml",
            13,
        ),
    ] {
        let git_ref = if matches!(role, "source" | "draft" | "signer") {
            format!("refs/tags/sdk-preview-{SOURCE}")
        } else {
            "refs/heads/main".to_owned()
        };
        write!(
            &mut lock,
            "{role}_workflow_path = \"{workflow}\"\n{role}_workflow_ref = \"tailrocks/velnor-new/{workflow}@{git_ref}\"\n{role}_workflow_sha = \"{SOURCE}\"\n{role}_run_id = {id}\n{role}_run_attempt = 1\n"
        )?;
    }
    write!(
        &mut lock,
        "payload_target = \"x86_64-unknown-linux-gnu\"\npayload_name = \"velnor-actions-x86_64-unknown-linux-gnu\"\npayload_size = 12345\npayload_sha256 = \"{}\"\nreceipt_sha256 = \"{RECEIPT}\"\nbundle_sha256 = \"{}\"\n",
        "c".repeat(64),
        "d".repeat(64)
    )?;
    write!(
        &mut lock,
        "producer_run_id = 20\nproducer_run_attempt = 1\nproducer_payload_artifact_id = 21\nproducer_payload_archive_sha256 = \"{}\"\ncandidate_source_artifact_id = 22\ncandidate_payload_artifact_id = 23\npublisher_run_attempt = 1\npublisher_job_id = 24\n",
        "e".repeat(64)
    )?;
    fs::write(&path, lock)?;
    let prepared = prepare(root)?;
    if prepared.config.checks.iter().any(|check| {
        check.selection == velnor_actions_contract::config::CheckSelection::DeclaredInputs
    }) {
        let output = tempfile::tempdir()?;
        let destination = output.path().join("preview");
        velnor_actions_orchestrator::generate(
            &prepared,
            &velnor_actions_orchestrator::GenerateOptions {
                output_dir: Some(destination.clone()),
            },
        )?;
        assert!(destination.join(".github/workflows/ci.yml").is_file());
    }
    let tree = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("missing preview workflow")?;
    let setup = tree
        .get(".github/actions/velnor-preview-setup/action.yml")
        .ok_or("missing protected setup")?;
    assert!(setup.contains("Stage protected preview launcher"));
    let yaml: Value = serde_saphyr::from_str(workflow)?;
    assert_identity_steps(&tree, &yaml)?;
    fs::write(path, original)?;
    Ok(())
}

fn assert_identity_steps(
    tree: &velnor_actions_workflow_renderer::render::RenderedTree,
    yaml: &Value,
) -> Result<(), Box<dyn Error>> {
    for id in [
        "plan",
        "required",
        "check-maintenance-helpers__hosted",
        "check-maintenance-helpers__local",
    ] {
        let steps = yaml["jobs"][id]["steps"]
            .as_array()
            .ok_or_else(|| format!("{id}: missing steps"))?;
        let mut flattened = Vec::new();
        for step in steps {
            if step["uses"] == "./.github/actions/check-maintenance-helpers" {
                let action = tree
                    .get(".github/actions/check-maintenance-helpers/action.yml")
                    .ok_or("missing shared check action")?;
                let composite: Value = serde_saphyr::from_str(action)?;
                flattened.extend(
                    composite["runs"]["steps"]
                        .as_array()
                        .ok_or("missing composite steps")?
                        .iter()
                        .cloned(),
                );
            } else {
                flattened.push(step.clone());
            }
        }
        let steps = &flattened;
        let setup_index = steps
            .iter()
            .position(|step| step["uses"] == "./.github/actions/velnor-preview-setup")
            .ok_or_else(|| format!("{id}: missing protected setup: {steps:?}"))?;
        let identity_index = steps
            .iter()
            .position(|step| {
                let text = step.to_string();
                [
                    "source-preview",
                    "0.1.75",
                    "x86_64-unknown-linux-gnu",
                    SOURCE,
                    RECEIPT,
                    &"c".repeat(64),
                ]
                .iter()
                .all(|value| text.contains(value))
            })
            .ok_or_else(|| format!("{id}: missing complete selected identity"))?;
        assert!(
            setup_index < identity_index,
            "{id}: identity used before setup"
        );
    }
    Ok(())
}
