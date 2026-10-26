// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::Step;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::verifier_builders;

use crate::OrchestratorError;

pub(crate) const VERIFY_JACTIONLINT_STEP: &str = verifier_builders::VERIFY_JACTIONLINT_STEP;

pub(crate) fn script(catalog: &ToolCatalog) -> String {
    verifier_builders::jactionlint_verifier_script(catalog)
}

pub(crate) fn argv(catalog: &ToolCatalog) -> Vec<String> {
    verifier_builders::jactionlint_verifier_argv(catalog)
}

pub(crate) fn verify_jactionlint_step(_catalog: &ToolCatalog) -> Step {
    verifier_builders::verify_jactionlint_step()
}

pub(crate) fn verifier_shell_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    verifier_builders::jactionlint_verifier_shell_step(catalog).map_err(OrchestratorError::from)
}
