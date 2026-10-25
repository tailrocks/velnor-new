use velnor_actions_contract::{Step, StepKind, StepRole};

pub(super) fn peel_checkout<'a>(
    steps: &'a [Step],
    checkout_uses: &str,
) -> Option<(&'a Step, &'a [Step])> {
    let (checkout, remaining) = steps.split_first()?;
    if !velnor_actions_contract::workflow::step_identity::is_configured_checkout(
        checkout,
        checkout_uses,
    ) || remaining.iter().any(is_checkout_step)
    {
        return None;
    }
    Some((checkout, remaining))
}

fn is_checkout_step(step: &Step) -> bool {
    step.role == Some(StepRole::Checkout)
        || matches!(
            &step.kind,
            StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@")
        )
}
