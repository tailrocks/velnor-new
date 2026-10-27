// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::bun_private_credentials_support::private_bun_repo;
use super::support::rendered_job_block;
use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_orchestrator::{prepare, render_staged_tree_with};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const REPOSITORY_CHECK: &str = "bun-assets-repository-check-v1";

#[test]
fn private_asset_handoffs_require_current_private_repository() -> Result<(), Box<dyn Error>> {
    let repo = private_bun_repo()?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree_with(&prepared, None)?;
    let workflow = rendered.get(WORKFLOW_PATH).ok_or("workflow is missing")?;
    for member in ["bun-fixture-ui", "bun-fixture-web"] {
        let producer_id = format!("{member}--assets");
        let producer = prepared
            .workflow
            .ir
            .jobs
            .get(&producer_id)
            .ok_or("producer is missing")?;
        assert_private_repository_admission(&producer_id, &producer.steps, workflow)?;
    }
    for consumer in [
        "bun-fixture-ui",
        "bun-fixture-web",
        "bun-fixture-web--unit",
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
    ] {
        assert!(!rendered_job_block(workflow, consumer)?.contains(REPOSITORY_CHECK));
    }
    Ok(())
}

fn assert_private_repository_admission(
    producer_id: &str,
    steps: &[Step],
    workflow: &str,
) -> Result<(), Box<dyn Error>> {
    let checks = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match &step.kind {
            StepKind::Internal { operation, env } if operation == REPOSITORY_CHECK => {
                Some((index, step, env))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        checks.len(),
        2,
        "{producer_id} can fetch or publish private assets without checking current repository visibility"
    );
    let mint = steps
        .iter()
        .position(|step| step.role == Some(StepRole::PrivateTokenMint))
        .ok_or("mint is missing")?;
    let export = steps.iter().position(|step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == "bun-assets-export-v1")).ok_or("export is missing")?;
    let upload = steps.iter().position(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@"))).ok_or("upload is missing")?;
    assert!(checks[0].0 < mint);
    assert!(export < checks[1].0 && checks[1].0 < upload);
    for (_, step, env) in checks {
        assert!(step.condition.is_none());
        assert!(!step.parallel);
        assert_eq!(
            env.get("GH_TOKEN").map(String::as_str),
            Some("${{ github.token }}")
        );
        assert_eq!(
            env.get("VELNOR_BUN_ASSET_REPOSITORY").map(String::as_str),
            Some("${{ github.repository }}")
        );
        assert!(
            !env.values()
                .any(|value| value.contains("FIXTURE_APP_PRIVATE_KEY")
                    || value.contains("steps.deps-token"))
        );
    }
    assert_eq!(
        rendered_job_block(workflow, producer_id)?
            .matches(REPOSITORY_CHECK)
            .count(),
        2
    );
    Ok(())
}
