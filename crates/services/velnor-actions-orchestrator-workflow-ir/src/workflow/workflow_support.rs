//! Policy-derived support validator selection.

use velnor_actions_contract_config::{
    ValidatorKind, VelnorConfig, VelnorSupportWorkflow, WorkflowPolicy,
};
use velnor_actions_orchestrator_discovery::discover::Discovery;

/// Build the support validator set for the selected repository policy.
pub(super) fn support_workflow(
    config: &VelnorConfig,
    discovery: &Discovery,
) -> Option<VelnorSupportWorkflow> {
    let policy = config.workflow.policy;
    let validation = config.workflow.generator_validation;
    let rust_policy = config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.policy.clone());
    let mut support = match policy {
        WorkflowPolicy::ConsumerV1 => {
            let mut validators = Vec::new();
            if rust_policy.is_some() && super::plan_uses_rust(discovery) {
                validators.push(ValidatorKind::Alint);
            }
            if config
                .workflow
                .verify
                .jobs
                .iter()
                .any(|job| job == "zizmor")
            {
                validators.push(ValidatorKind::Zizmor);
            }
            if validators.is_empty() {
                return None;
            }
            VelnorSupportWorkflow {
                validators,
                candidate_validation: false,
            }
        }
        WorkflowPolicy::VelnorRepositoryV1 => policy.support_workflow(validation),
    };
    if discovery.workspaces.is_empty() {
        support
            .validators
            .retain(|validator| *validator != ValidatorKind::CargoDeny);
    }
    if velnor_actions_orchestrator_provisioning::vectors::machete_crate_dirs(discovery).is_empty() {
        support
            .validators
            .retain(|validator| *validator != ValidatorKind::CargoMachete);
    }
    Some(support)
}
