//! Consumer planning boundary: authenticated analysis first, Cargo on a miss.

use velnor_actions_contract::{Job, Step, StepId, StepKind};

use crate::{RenderError, steps};

/// Stable binding for the typed early planning result.
pub const EARLY_PLAN_STEP_ID: &str = "early_plan";
/// A miss requires full preparation. An absent output cannot bypass it.
pub const NEEDS_CARGO_CONDITION: &str = "steps.early_plan.outputs.needs_cargo != 'false'";

/// Invoke the trusted helper using the ordinary plan request.
/// # Errors
/// Rejects malformed fixed operation or step identity.
pub fn early_plan_step() -> Result<Step, RenderError> {
    let mut step = steps::internal_step("Plan before Cargo", steps::EARLY_PLAN_OPERATION)?;
    step.id = Some(StepId::new(EARLY_PLAN_STEP_ID).map_err(RenderError::Contract)?);
    Ok(step)
}

/// Whether this job owns the consumer early planning boundary.
#[must_use]
pub fn has_early_plan(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Internal { operation } if operation == steps::EARLY_PLAN_OPERATION)
    })
}

/// Preserve an existing policy guard while requiring Cargo fallback.
pub fn require_cargo(step: &mut Step) {
    step.condition = Some(step.condition.as_ref().map_or_else(
        || NEEDS_CARGO_CONDITION.to_owned(),
        |condition| format!("({condition}) && ({NEEDS_CARGO_CONDITION})"),
    ));
}

#[cfg(test)]
mod tests {
    use super::{EARLY_PLAN_STEP_ID, NEEDS_CARGO_CONDITION, early_plan_step, require_cargo};
    use crate::steps;

    #[test]
    fn early_operation_reuses_the_normal_request_without_becoming_output_anchor() {
        let step = early_plan_step().expect("fixed step");
        assert_eq!(
            step.id.as_ref().map(|id| id.as_str()),
            Some(EARLY_PLAN_STEP_ID)
        );
        assert_eq!(
            steps::split_internal_operation(steps::EARLY_PLAN_OPERATION).expect("fixed operation"),
            (steps::EARLY_PLAN_OPERATION, steps::PLAN_OPERATION)
        );
        assert_ne!(EARLY_PLAN_STEP_ID, crate::PLAN_STEP_ID);
    }

    #[test]
    fn fallback_retains_trusted_writer_policy_and_missing_output_executes() {
        let mut step = steps::plan_step();
        require_cargo(&mut step);
        assert_eq!(step.condition.as_deref(), Some(NEEDS_CARGO_CONDITION));
        step.condition = Some("success() && github.ref_protected == true".to_owned());
        require_cargo(&mut step);
        assert_eq!(
            step.condition.as_deref(),
            Some(
                "(success() && github.ref_protected == true) && (steps.early_plan.outputs.needs_cargo != 'false')"
            )
        );
    }
}
