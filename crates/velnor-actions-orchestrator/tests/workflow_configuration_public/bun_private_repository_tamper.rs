// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::bun_private_credentials_support::private_bun_repo;
use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

const CHECK: &str = "bun-assets-repository-check-v1";

#[test]
fn private_asset_repository_checks_cannot_be_removed_or_weakened_by_job_edits()
-> Result<(), Box<dyn Error>> {
    for check_number in 0..2 {
        for mutation in ["remove", "condition", "binding"] {
            let repo = private_bun_repo()?;
            let mut prepared = prepare(repo.path())?;
            let policy = prepared
                .workflow
                .context
                .workflow_tasks
                .iter()
                .find_map(|task| match task {
                    WorkflowTaskPolicy::BunAssets(policy)
                        if policy.id == "bun-fixture-web--assets" =>
                    {
                        Some(policy)
                    }
                    _ => None,
                })
                .ok_or("producer policy is missing")?;
            assert!(!policy.steps.iter().any(|step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == CHECK)));
            let job = prepared
                .workflow
                .ir
                .jobs
                .get_mut("bun-fixture-web--assets")
                .ok_or("producer job is missing")?;
            let index = job
                .steps
                .iter()
                .enumerate()
                .filter_map(|(index, step)| {
                    matches!(&step.kind, StepKind::Internal { operation, .. } if operation == CHECK)
                        .then_some(index)
                })
                .nth(check_number)
                .ok_or("canonical privacy check is missing")?;
            match mutation {
                "remove" => {
                    job.steps.remove(index);
                }
                "condition" => job.steps[index].condition = Some("false".to_owned()),
                "binding" => match &mut job.steps[index].kind {
                    StepKind::Internal { env, .. } => {
                        env.insert(
                            "VELNOR_BUN_ASSET_REPOSITORY".to_owned(),
                            "fixture-owner/another-private-repository".to_owned(),
                        );
                    }
                    _ => return Err("privacy check changed kind".into()),
                },
                _ => return Err("unknown mutation".into()),
            }
            let error = render_staged_tree_with(&prepared, None)
                .err()
                .ok_or("altered privacy check must fail native job validation")?;
            assert!(
                error.to_string().contains("bun_assets_job_contract")
                    || error
                        .to_string()
                        .contains("bun_assets_repository_check_scope"),
                "unexpected {mutation} rejection: {error}"
            );
        }
    }
    Ok(())
}
