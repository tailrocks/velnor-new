use velnor_actions_contract::Step;
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
    inline.is_some_and(|verifiers| verifiers.is_installed_rust_inline(step))
}

pub(super) fn restored_rust(step: &Step, inline: Option<&VerifierSteps>) -> bool {
    inline.is_some_and(|verifiers| verifiers.is_restored_rust_inline(step))
}

pub(super) fn jactionlint(step: &Step, inline: Option<&VerifierSteps>) -> bool {
    inline.is_some_and(|verifiers| verifiers.is_jactionlint_inline(step))
}
