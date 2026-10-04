//! Sole owner reconstruction of the complete release workflow envelope.
use super::release_steps::JobInputs;
use super::{helper_approval, release_steps, release_support_sources, release_triggers};
use crate::OrchestratorError;
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_spec::BootstrapPlan;

/// Reconstruct from frozen generation inputs, never a candidate workflow.
pub(crate) fn compile(inputs: &JobInputs<'_>) -> Result<ReleaseWorkflowSpec, OrchestratorError> {
    super::require_source_intent_qualification()?;
    let bootstrap = BootstrapPlan {
        plan_id: super::release_identity::plan_id_for_source(inputs.sha),
        repository: inputs.repository.to_owned(),
        source_sha: inputs.sha.to_owned(),
        registry: inputs.actual_registry.to_owned(),
        packages: inputs.packages.clone(),
        version: inputs
            .release
            .bootstrap
            .as_ref()
            .map(|record| record.version.clone()),
    };
    let jobs = release_steps::assemble_jobs(inputs)?;
    let registry = release_steps::helper_registry(inputs)?;
    let sources =
        release_support_sources::complete_support_sources(inputs, env!("CARGO_PKG_VERSION"))?;
    let (helper_registry, support_sources) =
        helper_approval::approve(inputs, registry, sources, env!("CARGO_PKG_VERSION"))?
            .into_parts();
    Ok(ReleaseWorkflowSpec {
        name: super::RELEASE_WORKFLOW_NAME.to_owned(),
        repository: inputs.repository.to_owned(),
        triggers: release_triggers(inputs.branch, &bootstrap),
        concurrency: super::release_identity::release_concurrency(
            inputs.actual_registry,
            inputs.repository,
            &inputs.release.manifest_path,
        )?,
        jobs,
        preparation_enabled: inputs.release.release_pr,
        helper_registry,
        support_sources,
        bootstrap_tools: inputs.bootstrap_tools.clone(),
        bootstrap,
        reconciliation: inputs.reconciliation.clone(),
        publish_environment: inputs.release.environment.clone(),
        bootstrap_environment: inputs.release.environment.clone(),
    })
}
