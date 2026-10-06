//! Authoritative release acquisition factory; caller records cannot grant approval.
use crate::{OrchestratorError, prepare::GenerationPreparation};
use velnor_actions_workflow_renderer::MiseSetup;

/// Source qualification comes from the generator factory, never caller pins.
pub(super) fn approved_bootstrap(
    prep: &GenerationPreparation,
    supplied: &MiseSetup,
) -> Result<
    velnor_actions_workflow_renderer::release_bootstrap::ReleaseBootstrapApproval,
    OrchestratorError,
> {
    let authoritative = crate::pins::resolve_mise_setup(&prep.config, &prep.runner_label)?;
    if supplied != &authoritative {
        return Err(OrchestratorError::Contract {
            problem: "release_bootstrap_not_generator_approved".to_owned(),
        });
    }
    Ok(
        velnor_actions_workflow_renderer::release_bootstrap::ReleaseBootstrapApproval {
            checkout_uses: velnor_actions_actionlint::actions::PinnedActionRef::checkout()
                .uses_value(),
            mise: authoritative,
        },
    )
}
