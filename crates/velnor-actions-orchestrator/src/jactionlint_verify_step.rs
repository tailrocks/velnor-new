use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

pub(crate) const VERIFY_JACTIONLINT_STEP: &str = "Verify jactionlint";

pub(crate) fn script(catalog: &ToolCatalog) -> String {
    velnor_actions_workflow_renderer::jactionlint_verifier_script(
        catalog.version(velnor_actions_mise::PinnedTool::Jactionlint),
    )
}

pub(crate) fn argv(catalog: &ToolCatalog) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), script(catalog)]
}

pub(crate) fn verify_jactionlint_step(_catalog: &ToolCatalog) -> Step {
    crate::workflow::verifier_actions::call_step(
        VERIFY_JACTIONLINT_STEP,
        velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
        Some(velnor_actions_contract::StepRole::VerifyJactionlint),
    )
}

pub(crate) fn verifier_shell_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::steps::jactionlint_verifier_step(
        VERIFY_JACTIONLINT_STEP,
        argv(catalog),
        BTreeMap::new(),
    )
    .map_err(OrchestratorError::from)
}
