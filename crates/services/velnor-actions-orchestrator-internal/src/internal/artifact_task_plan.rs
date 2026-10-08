//! Resolve provider selection for planned artifact tasks.

use velnor_actions_contract_config::ExecutionMode;
use velnor_actions_contract_workflow::ArtifactBuildProvider;

/// Resolve the exact provider inventory for artifact tasks from effective routing.
pub(super) fn artifact_providers(
    config: &velnor_actions_contract_config::VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Vec<ArtifactBuildProvider> {
    if config.workflow.artifact_tasks.is_empty() {
        return Vec::new();
    }
    let configured = config
        .execution
        .as_ref()
        .map_or(ExecutionMode::Hosted, |execution| {
            execution.mode.unwrap_or_else(|| {
                if execution.default_profile == execution.scale_set_profile {
                    ExecutionMode::ScaleSet
                } else {
                    ExecutionMode::Hosted
                }
            })
        });
    let mode = dispatch.unwrap_or(configured);
    match mode {
        ExecutionMode::Hosted => vec![ArtifactBuildProvider::GithubHosted],
        ExecutionMode::ScaleSet => vec![ArtifactBuildProvider::VelnorScaleSet],
        ExecutionMode::Both => vec![
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    }
}
