//! Receipt preparation needs source-issued transport binding, beyond a registry record.
use crate::RenderError;
use velnor_actions_contract::{HelperInvocation, SourceBoundOperation, Step, StepKind};

pub(crate) fn validate_steps<'a>(steps: impl Iterator<Item = &'a Step>) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::SourceBoundHelper { invocation, .. } = &step.kind {
            validate_invocation(invocation)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_invocation(invocation: &HelperInvocation) -> Result<(), RenderError> {
    if invocation.descriptor().operation() == SourceBoundOperation::ReceiptOwnedPreparation {
        // The source owner cannot mint a qualified quarantine restore binding yet.
        // Registry membership alone does not bind the complete transport recipe.
        return Err(RenderError::InvalidWorkflow(
            "receipt_preparation_requires_owned_binding".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "receipt_preparation_admission_tests.rs"]
mod tests;
