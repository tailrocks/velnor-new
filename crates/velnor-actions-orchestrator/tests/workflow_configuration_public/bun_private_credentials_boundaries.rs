// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::bun_private_credentials_support::{
    assert_member_handoff, assert_runner, private_bun_repo, set_execution,
};
use super::support::rendered_job_block;
use velnor_actions_contract::{
    BUN_ASSETS_IMPORT_OPERATION, BUN_ASSETS_PREPARE_OPERATION, StepKind, StepRole,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::{
    render::WORKFLOW_PATH, verification_jobs::WorkflowTaskPolicy,
};

const HOSTED: &str = "ubuntu-26.04";
const SCALE_SET: &str = "[velnor, ubuntu-26.04-scale-set]";
const APP_KEY: &str = "FIXTURE_APP_PRIVATE_KEY";

#[test]
fn mixed_default_and_member_overrides_keep_private_producers_hosted() -> Result<(), Box<dyn Error>>
{
    let repo = private_bun_repo()?;
    set_execution(
        repo.path(),
        "local",
        &[("bun-fixture-ui", "hosted"), ("bun-fixture-web", "local")],
    )?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree_with(&prepared, None)?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;

    assert_runner(workflow, "bun-fixture-ui", HOSTED)?;
    assert_runner(workflow, "bun-fixture-web", SCALE_SET)?;
    for member_id in ["bun-fixture-ui", "bun-fixture-web"] {
        let producer_id = format!("{member_id}--assets");
        assert_runner(workflow, &producer_id, HOSTED)?;
        let producer = rendered_job_block(workflow, &producer_id)?;
        assert!(producer.contains(APP_KEY));
        assert_member_handoff(workflow, member_id)?;
    }
    assert_runner(workflow, "bun-fixture-web--unit", SCALE_SET)?;
    assert_runner(workflow, "bun-fixture-web--browser", SCALE_SET)?;
    assert_runner(workflow, "bun-fixture-web--browser--report", SCALE_SET)?;
    assert!(!workflow.contains("bun-fixture-web__hosted:"));
    assert!(!workflow.contains("bun-fixture-web__local:"));
    assert_eq!(
        workflow.matches("  bun-fixture-web:\n").count(),
        1,
        "mixed routing must keep one logical member check"
    );
    assert!(!rendered_job_block(workflow, "bun-fixture-web")?.contains(APP_KEY));
    Ok(())
}

#[test]
fn explicit_local_private_asset_producer_override_is_rejected() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    set_execution(
        repo.path(),
        "hosted",
        &[("bun-fixture-web--assets", "local")],
    )?;
    let error = match prepare(repo.path()) {
        Ok(prepared) => render_staged_tree_with(&prepared, None)
            .expect_err("a local private asset producer override must fail closed")
            .to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("execution.overrides.bun-fixture-web--assets.profile"));
    assert!(error.contains("bun_private_assets_hosted_only"));
    Ok(())
}

#[test]
fn renderer_rejects_missing_member_asset_import() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let member = prepared
        .workflow
        .ir
        .jobs
        .get_mut("bun-fixture-web")
        .ok_or("prepared Bun member job is missing")?;
    member.steps.retain(|step| {
        !matches!(
            &step.kind,
            StepKind::Internal { operation, .. }
                if operation == BUN_ASSETS_IMPORT_OPERATION
        )
    });
    let error = render_staged_tree_with(&prepared, None)
        .expect_err("a member without its verified import must fail closed")
        .to_string();
    assert!(error.contains("bun_member_job_contract:bun-fixture-web"));
    Ok(())
}

#[test]
fn renderer_rejects_member_without_its_asset_producer_dependency() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let member = prepared
        .workflow
        .ir
        .jobs
        .get_mut("bun-fixture-web")
        .ok_or("prepared Bun member job is missing")?;
    member
        .needs
        .retain(|need| need != "bun-fixture-web--assets");
    let error = render_staged_tree_with(&prepared, None)
        .expect_err("a member without its producer dependency must fail closed")
        .to_string();
    assert!(error.contains("native_authority_graph"), "{error}");
    Ok(())
}

#[test]
fn renderer_rejects_conditionally_skipped_member_asset_import() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let steps = bun_member_steps(&mut prepared)?;
    let import_index = steps
        .iter()
        .position(|step| {
            matches!(
                &step.kind,
                StepKind::Internal { operation, .. }
                    if operation == BUN_ASSETS_IMPORT_OPERATION
            )
        })
        .ok_or("prepared Bun member import step is missing")?;
    steps[import_index].condition = Some("false".to_owned());
    mirror_bun_member_policy_steps(&mut prepared)?;

    let error = match render_staged_tree_with(&prepared, None) {
        Ok(_) => return Err("renderer accepted a conditionally skipped asset import".into()),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("bun_consumer_asset_handoff_malformed:bun-fixture-web"),
        "unexpected renderer error: {error}"
    );
    Ok(())
}

#[test]
fn renderer_rejects_conditionally_skipped_member_asset_prepare() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let steps = bun_member_steps(&mut prepared)?;
    let prepare_index = steps
        .iter()
        .position(|step| {
            matches!(
                &step.kind,
                StepKind::Internal { operation, .. }
                    if operation == BUN_ASSETS_PREPARE_OPERATION
            )
        })
        .ok_or("prepared Bun member asset preparation step is missing")?;
    steps[prepare_index].condition = Some("false".to_owned());
    mirror_bun_member_policy_steps(&mut prepared)?;

    assert_member_handoff_rejected(&prepared, "a conditionally skipped asset prepare")
}

#[test]
fn renderer_rejects_conditionally_skipped_member_asset_download() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let steps = bun_member_steps(&mut prepared)?;
    let download_index = steps
        .iter()
        .position(|step| {
            matches!(
                &step.kind,
                StepKind::Action { uses, with, .. }
                    if uses.starts_with("actions/download-artifact@")
                        && with.get("name").is_some_and(|name| name.starts_with("velnor-pw-assets-"))
            )
        })
        .ok_or("prepared Bun member asset download step is missing")?;
    steps[download_index].condition = Some("false".to_owned());
    mirror_bun_member_policy_steps(&mut prepared)?;

    assert_member_handoff_rejected(&prepared, "a conditionally skipped asset download")
}

#[test]
fn renderer_rejects_registry_install_before_member_asset_handoff() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let steps = bun_member_steps(&mut prepared)?;
    let prepare_index = steps
        .iter()
        .position(|step| {
            matches!(
                &step.kind,
                StepKind::Internal { operation, .. }
                    if operation == BUN_ASSETS_PREPARE_OPERATION
            )
        })
        .ok_or("prepared Bun member asset preparation step is missing")?;
    let registry_index = steps
        .iter()
        .position(|step| step.role == Some(StepRole::BunRegistryInstall))
        .ok_or("prepared Bun member registry install step is missing")?;
    let registry = steps.remove(registry_index);
    steps.insert(prepare_index.min(registry_index), registry);
    mirror_bun_member_policy_steps(&mut prepared)?;

    let error = match render_staged_tree_with(&prepared, None) {
        Ok(_) => {
            return Err(
                "renderer accepted registry installation before the verified handoff".into(),
            );
        }
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("bun_consumer_asset_handoff_malformed:bun-fixture-web"),
        "unexpected renderer error: {error}"
    );
    Ok(())
}

#[test]
fn renderer_rejects_unrecognized_setup_before_member_asset_handoff() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let steps = bun_member_steps(&mut prepared)?;
    let prepare_index = steps
        .iter()
        .position(|step| {
            matches!(&step.kind,
            StepKind::Internal { operation, .. } if operation == BUN_ASSETS_PREPARE_OPERATION)
        })
        .ok_or("prepared Bun member asset preparation step is missing")?;
    let setup = velnor_actions_workflow_renderer::steps::shell_step(
        "Run setup",
        vec!["bash".to_owned(), "-c".to_owned(), "bun install".to_owned()],
        std::collections::BTreeMap::default(),
    )?;
    steps.insert(prepare_index, setup);
    mirror_bun_member_policy_steps(&mut prepared)?;
    assert_member_handoff_rejected(&prepared, "unrecognized setup before the verified handoff")
}

fn bun_member_steps(
    prepared: &mut velnor_actions_orchestrator::GenerationPreparation,
) -> Result<&mut Vec<velnor_actions_contract::Step>, Box<dyn Error>> {
    prepared
        .workflow
        .context
        .workflow_tasks
        .iter_mut()
        .find_map(|task| match task {
            WorkflowTaskPolicy::Bun(policy) if policy.id == "bun-fixture-web" => {
                Some(&mut policy.steps)
            }
            _ => None,
        })
        .ok_or_else(|| "prepared Bun member policy is missing".into())
}

fn mirror_bun_member_policy_steps(
    prepared: &mut velnor_actions_orchestrator::GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let policy_steps = prepared
        .workflow
        .context
        .workflow_tasks
        .iter()
        .find_map(|task| match task {
            WorkflowTaskPolicy::Bun(policy) if policy.id == "bun-fixture-web" => {
                Some(policy.steps.clone())
            }
            _ => None,
        })
        .ok_or("prepared Bun member policy is missing")?;
    let member = prepared
        .workflow
        .ir
        .jobs
        .get_mut("bun-fixture-web")
        .ok_or("prepared Bun member job is missing")?;
    let checkout = member
        .steps
        .first()
        .cloned()
        .ok_or("prepared Bun member checkout step is missing")?;
    member.steps = std::iter::once(checkout).chain(policy_steps).collect();
    Ok(())
}

fn assert_member_handoff_rejected(
    prepared: &velnor_actions_orchestrator::GenerationPreparation,
    reason: &str,
) -> Result<(), Box<dyn Error>> {
    let error = match render_staged_tree_with(prepared, None) {
        Ok(_) => return Err(format!("renderer accepted {reason}").into()),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("bun_consumer_asset_handoff_malformed:bun-fixture-web"),
        "unexpected renderer error: {error}"
    );
    Ok(())
}
