// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use velnor_actions_contract::{ValidatorKind, VerifyConfig};
use velnor_actions_workflow_renderer::native_authority::support_commands::{
    self, SupportCommandError,
};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

pub(crate) use support_commands::verify::validator_install_pin;
pub(crate) fn verify_kinds(jobs: &[String]) -> Result<Vec<ValidatorKind>, OrchestratorError> {
    support_commands::verify::verify_kinds(jobs).map_err(map_error)
}
pub(crate) fn push_verify_commands(
    commands: &mut Vec<ValidatorCommand>,
    verify: &[ValidatorKind],
    options: &VerifyConfig,
) -> Result<(), OrchestratorError> {
    support_commands::verify::push_verify_commands(commands, verify, options).map_err(map_error)
}
pub(crate) fn map_error(error: SupportCommandError) -> OrchestratorError {
    match error {
        SupportCommandError::Contract { problem } => OrchestratorError::Contract { problem },
        SupportCommandError::Render(error) => error.into(),
    }
}
