use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;

use crate::workflow::verifier_actions::{VerifierSteps, verifier_steps};

pub(super) fn build(catalog: &ToolCatalog, blocking: &mut Vec<String>) -> Option<VerifierSteps> {
    match verifier_steps(&catalog.for_emission()) {
        Ok(steps) => Some(steps),
        Err(error) => {
            blocking.push(format!("verifier_step_templates_invalid:{error}"));
            None
        }
    }
}

pub(super) fn installed_rust(step: &Step, inline: Option<&VerifierSteps>) -> bool {
    has_action(
        step,
        StepRole::VerifyRustManifest,
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
    ) || inline.is_some_and(|verifiers| verifiers.is_installed_rust_inline(step))
}

pub(super) fn jactionlint(step: &Step, inline: Option<&VerifierSteps>) -> bool {
    has_action(
        step,
        StepRole::VerifyJactionlint,
        velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
    ) || inline.is_some_and(|verifiers| verifiers.is_jactionlint_inline(step))
}

fn has_action(step: &Step, role: StepRole, expected_uses: &str) -> bool {
    step.role == Some(role)
        && step.id.is_none()
        && step.condition.is_none()
        && !step.parallel
        && matches!(
            &step.kind,
            StepKind::Action { uses, with, env }
                if uses == expected_uses && with.is_empty() && env.is_empty()
        )
}
