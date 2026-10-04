//! Validated pinned action and checkout step construction.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use super::scan_for_private_subcommands;
use crate::RenderError;

/// Validate an `owner/repo@<40 hex>` action ref; branches are rejected.
/// # Errors
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    let Some((name, sha)) = uses.split_once('@') else {
        return Err(RenderError::BadActionRef(format!("missing_sha:{uses}")));
    };
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    };
    if owner.is_empty() || repo.is_empty() || !is_action_name(name) {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    }
    if name.starts_with("actions/setup-")
        || matches!(name, "taiki-e/install-action" | "jdx/mise-action")
    {
        return Err(RenderError::BadActionRef(format!(
            "forbidden_action:{uses}"
        )));
    }
    if !velnor_actions_contract::ids::is_lower_hex_len(sha, 40) {
        return Err(RenderError::BadActionRef(format!("unpinned_ref:{uses}")));
    }
    Ok(())
}

/// Checkout step without persisted credentials.
/// # Errors
pub fn checkout_step(uses: &str) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!("not_checkout:{uses}")));
    }
    let with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    action_step("Checkout", uses, with)
}

/// Validated pinned-action step.
///
/// Names share the step-name gate (no expressions); `with:` keys never
/// carry expressions and values only allowlisted runner spans.
/// # Errors
pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    action_step_with_env(name, uses, with, BTreeMap::new())
}

/// Validated action step with step-level environment.
///
/// Same gates as [`action_step`]; `env` renders as the step's `env:`
/// map and applies to the action's main and post phases alike, which
/// is what lets a cache mode gate the post-step save while the
/// restore still runs on every event.
/// # Errors
pub fn action_step_with_env(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    validate_uses(uses)?;
    scan_for_private_subcommands(name)?;
    scan_for_private_subcommands(uses)?;
    for (key, value) in &with {
        crate::expressions::check_with_key(key)?;
        crate::expressions::check_with_value(key, value)?;
        scan_for_private_subcommands(key)?;
        scan_for_private_subcommands(value)?;
    }
    crate::commands::validate_env(&env)?;
    Ok(Step {
        id: None,
        name: name.to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env,
        },
    })
}

/// True for `owner/repo` over alphanumerics plus `.-_`.
fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}
