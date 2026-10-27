// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{ExecutionMode, expand_workflow_with_bun_plan};
use velnor_actions_orchestrator::{migrate_config, prepare};
use velnor_actions_workflow_renderer::{
    MiseSetup, render::render_workflow_ir_strict, setup::MISE_BINARY_SHA256_LINUX_X64,
};

use super::bun_planning_fixture::{ChangedMember, make_bun_planning_repo};
use super::bun_routing::make_bun_repo;

const BUN_JOBS: [&str; 5] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    "bun-fixture-web--browser",
    "bun-fixture-web--browser--report",
    "bun-fixture-web--unit",
];

#[test]
fn public_expansion_preserves_bun_context_for_strict_rendering() -> Result<(), Box<dyn Error>> {
    for schema in [1, 2] {
        let repo = make_bun_repo()?;
        if schema == 2 {
            migrate_config(repo.path(), 2, true)?;
        }
        let prepared = prepare(repo.path())?;
        for mode in [
            ExecutionMode::Hosted,
            ExecutionMode::ScaleSet,
            ExecutionMode::Both,
        ] {
            let expansion =
                expand_workflow_with_bun_plan(&prepared.workflow.ir, &prepared.config, Some(mode))?;
            let actual: Vec<_> = expansion
                .ir
                .jobs
                .keys()
                .filter(|id| id.starts_with("bun-"))
                .map(String::as_str)
                .collect();
            assert_eq!(actual, BUN_JOBS, "schema {schema} {mode:?}");
            let mut context = prepared.workflow.context.clone();
            context.bun_lane_plan = expansion.bun_lane_plan;
            context.scale_set_selector = if schema == 2 && mode != ExecutionMode::Hosted {
                Some(
                    prepared
                        .config
                        .execution
                        .as_ref()
                        .ok_or("schema-2 fixture has no execution configuration")?
                        .scale_selector()?,
                )
            } else {
                None
            };
            let rendered = render_workflow_ir_strict(
                &expansion.ir,
                prepared.config.workflow.policy,
                prepared.workflow.support.as_ref(),
                &context,
                &fixture_mise_setup(),
            );
            assert!(
                rendered.is_ok(),
                "public expansion lost required render context for schema {schema} {mode:?}: {rendered:?}"
            );
            assert!(rendered?.contains("bun-fixture-web"));
        }
    }
    Ok(())
}

#[test]
fn expanded_bun_jobs_gate_on_their_unsuffixed_plan_identity() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_planning_repo(ChangedMember::Web)?;
    let prepared = prepare(repo.repo.path())?;
    let expansion = expand_workflow_with_bun_plan(&prepared.workflow.ir, &prepared.config, None)?;
    for id in BUN_JOBS {
        let condition = expansion
            .ir
            .jobs
            .get(id)
            .and_then(|job| job.condition.as_deref())
            .ok_or_else(|| format!("expanded Bun job {id} has no condition"))?;
        if id == "bun-fixture-web--browser--report" {
            assert!(condition.contains("needs.bun-fixture-web--browser.result"));
            continue;
        }
        let expected = format!("!contains(needs.plan.outputs.not_affected_jobs, ',{id},')");
        assert!(condition.contains(&expected), "Bun job {id}: {condition}");
    }
    Ok(())
}

fn fixture_mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA,
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}
