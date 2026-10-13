//! Scheduled upstream-freshness emission (P12-4 wiring).
//!
//! Adds the generated `freshness.yml` schedule-only probe under the
//! Velnor-repository policy, where the Velnor-owned inventory and its
//! `scripts/check-freshness.sh` reader exist. Consumers carry neither,
//! so consumer trees omit the file instead of emitting a workflow
//! that references a missing script.

use velnor_actions_contract::{FRESHNESS_CRON_WEEKLY, ScheduleTrigger, WorkflowPolicy};
use velnor_actions_mise::catalog::RUST_VERSION;
use velnor_actions_workflow_renderer::freshness::{FreshnessSpec, render_freshness_workflow};
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::OrchestratorError;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

/// True when the freshness workflow is emitted for this preparation.
///
/// Single predicate shared by emission and plan listing, so the two
/// can never disagree about the file's presence.
pub(crate) fn freshness_enabled(prep: &GenerationPreparation) -> bool {
    prep.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1
}

/// Render the freshness workflow, or nothing outside the Velnor policy.
///
/// # Errors
///
/// Returns render errors for invalid spec scalars or unsupported runners.
pub(crate) fn freshness_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if !freshness_enabled(prep) {
        return Ok(Vec::new());
    }
    let mise_setup = crate::pins::resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let spec = FreshnessSpec {
        schedule: ScheduleTrigger {
            cron: vec![FRESHNESS_CRON_WEEKLY.to_owned()],
        },
        runs_on: prep.runner_label.clone(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        mise_setup,
        rust_version: RUST_VERSION.to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    Ok(vec![render_freshness_workflow(&spec)?])
}
