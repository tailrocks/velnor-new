// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{FRESHNESS_CRON_WEEKLY, ScheduleTrigger, WorkflowPolicy};
use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::freshness::{FreshnessSpec, render_freshness_workflow};
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::OrchestratorError;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

#[path = "upgrades_emit.rs"]
pub(crate) mod upgrades;

pub(crate) fn freshness_enabled(prep: &GenerationPreparation) -> bool {
    prep.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1
}

pub(crate) fn freshness_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if !freshness_enabled(prep) {
        return Ok(Vec::new());
    }
    let mise_setup = crate::pins::resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let emission = prep.emission_catalog();
    let spec = FreshnessSpec {
        schedule: ScheduleTrigger {
            cron: vec![FRESHNESS_CRON_WEEKLY.to_owned()],
        },
        runs_on: prep.runner_label.clone(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        mise_setup,
        rust_version: emission.version(PinnedTool::Rust).to_owned(),
        verify_rust_argv: crate::workflow::verify_rust_argv(&emission),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    Ok(vec![render_freshness_workflow(&spec)?])
}
