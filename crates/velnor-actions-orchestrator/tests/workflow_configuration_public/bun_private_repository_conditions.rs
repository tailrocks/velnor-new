// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::bun_private_credentials_support::private_bun_repo;
use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

#[test]
fn private_asset_upload_cannot_run_after_repository_admission_fails() -> Result<(), Box<dyn Error>>
{
    assert_producer_condition_rejected("upload", Some("always()"))
}

#[test]
fn private_asset_cleanup_cannot_lose_its_canonical_failure_condition() -> Result<(), Box<dyn Error>>
{
    assert_producer_condition_rejected("revoke", None)
}

fn assert_producer_condition_rejected(
    target: &str,
    condition: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let mut prepared = prepare(repo.path())?;
    let policy = prepared
        .workflow
        .context
        .workflow_tasks
        .iter_mut()
        .find_map(|task| match task {
            WorkflowTaskPolicy::BunAssets(policy) if policy.id == "bun-fixture-web--assets" => {
                Some(policy)
            }
            _ => None,
        })
        .ok_or("producer policy is missing")?;
    let policy_step = policy
        .steps
        .iter_mut()
        .find(|step| targeted_step(step, target))
        .ok_or("target policy step is missing")?;
    policy_step.condition = condition.map(str::to_owned);
    let producer = prepared
        .workflow
        .ir
        .jobs
        .get_mut("bun-fixture-web--assets")
        .ok_or("producer job is missing")?;
    let ir_step = producer
        .steps
        .iter_mut()
        .find(|step| targeted_step(step, target))
        .ok_or("target job step is missing")?;
    ir_step.condition = condition.map(str::to_owned);
    let error = render_staged_tree_with(&prepared, None).err().ok_or(
        "matching policy and job conditions must not bypass private asset admission or cleanup",
    )?;
    assert!(
        error
            .to_string()
            .contains("bun_assets_producer_condition:bun-fixture-web--assets"),
        "unexpected condition rejection: {error}"
    );
    Ok(())
}

fn targeted_step(step: &Step, target: &str) -> bool {
    match target {
        "upload" => {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@"))
        }
        "revoke" => step.role == Some(StepRole::PrivateRevoke),
        _ => false,
    }
}
