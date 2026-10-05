//! Action `uses` checks. One local tool-seed path is allowed. Every other
//! ref must be `owner/repo` at a 40-hex commit.

use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;

/// True only for the expected unconditional checkout action shape.
///
/// Display names are presentation. Cache and lane placement require the exact
/// configured, pinned action, credential setting, and empty environment.
pub(crate) fn is_configured_checkout(step: &Step, expected_uses: &str) -> bool {
    step.condition.is_none()
        && matches!(
            &step.kind,
            StepKind::Action { uses, with, env }
                if uses == expected_uses
                    && uses.starts_with("actions/checkout@")
                    && validate_uses(uses).is_ok()
                    && with.len() == 1
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
                    && env.is_empty()
        )
}

/// True for the broad credential-free checkout step used by shared lanes.
/// Authored inputs and environment stay on the outer job, not the composite.
pub(crate) fn is_checkout_action(step: &Step, expected_uses: &str) -> bool {
    step.condition.is_none()
        && matches!(
            &step.kind,
            StepKind::Action { uses, with, .. }
                if uses == expected_uses
                    && uses.starts_with("actions/checkout@")
                    && validate_uses(uses).is_ok()
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
        )
}

/// Validate an `owner/repo@<40 hex>` action ref. Branch names are rejected.
///
/// `./.github/actions/velnor-tool-seed` is the one local composite this
/// renderer emits. Every other local path is rejected.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for a missing pin, a malformed
/// name, a forbidden installer action, or a short ref.
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    if uses == crate::tool_seed::TOOL_SEED_USES {
        return Ok(());
    }
    let Some((name, sha)) = uses.split_once('@') else {
        return Err(RenderError::BadActionRef(format!("missing_sha:{uses}")));
    };
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    };
    if owner.is_empty() || repo.is_empty() || !is_action_name(name) {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    }
    if name.starts_with("actions/setup-") || name == "taiki-e/install-action" {
        return Err(RenderError::BadActionRef(format!(
            "forbidden_action:{uses}"
        )));
    }
    if !velnor_actions_contract::ids::is_lower_hex_len(sha, 40) {
        return Err(RenderError::BadActionRef(format!("unpinned_ref:{uses}")));
    }
    Ok(())
}

/// True for `owner/repo` over alphanumerics plus `.-_`.
fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::{is_checkout_action, is_configured_checkout};
    use crate::RenderError;
    use std::collections::BTreeMap;

    #[test]
    fn checkout_authority_uses_the_configured_action_shape_not_its_name() -> Result<(), RenderError>
    {
        let expected = format!("actions/checkout@{}", "a".repeat(40));
        let inputs = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
        let mut renamed = crate::steps::action_step("Source", &expected, inputs.clone())?;
        assert!(is_configured_checkout(&renamed, &expected));
        renamed.name = "Checkout".to_owned();
        assert!(is_configured_checkout(&renamed, &expected));

        let wrong_pin = format!("actions/checkout@{}", "b".repeat(40));
        let wrong = crate::steps::action_step("Checkout", &wrong_pin, inputs.clone())?;
        assert!(!is_configured_checkout(&wrong, &expected));
        let extra_input = crate::steps::action_step(
            "Checkout",
            &expected,
            BTreeMap::from([
                ("persist-credentials".to_owned(), "false".to_owned()),
                ("path".to_owned(), "workspace".to_owned()),
            ]),
        )?;
        assert!(!is_configured_checkout(&extra_input, &expected));
        let mut conditional = crate::steps::action_step("Checkout", &expected, inputs.clone())?;
        conditional.condition = Some("always()".to_owned());
        assert!(!is_configured_checkout(&conditional, &expected));
        let mut environment = crate::steps::action_step("Checkout", &expected, inputs)?;
        if let velnor_actions_contract::StepKind::Action { env, .. } = &mut environment.kind {
            env.insert("TOKEN".to_owned(), String::new());
        }
        assert!(!is_configured_checkout(&environment, &expected));
        let label_only =
            crate::steps::shell_step("Checkout", vec!["true".to_owned()], BTreeMap::new())?;
        assert!(!is_configured_checkout(&label_only, &expected));
        Ok(())
    }

    #[test]
    fn generic_checkout_preserves_extra_inputs_and_environment() -> Result<(), RenderError> {
        let expected = format!("actions/checkout@{}", "a".repeat(40));
        let mut checkout = crate::steps::action_step(
            "Source",
            &expected,
            BTreeMap::from([
                ("persist-credentials".to_owned(), "false".to_owned()),
                ("fetch-depth".to_owned(), "0".to_owned()),
            ]),
        )?;
        if let velnor_actions_contract::StepKind::Action { env, .. } = &mut checkout.kind {
            env.insert("FETCH_SUBMODULES".to_owned(), "true".to_owned());
        }
        assert!(is_checkout_action(&checkout, &expected));
        assert!(!is_configured_checkout(&checkout, &expected));
        Ok(())
    }
}
