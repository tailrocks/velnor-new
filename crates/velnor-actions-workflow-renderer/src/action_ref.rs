//! Action `uses` checks. Fixed local renderer actions are allowed; every other
//! ref must be `owner/repo` at a 40-hex commit.

use crate::RenderError;
use velnor_actions_contract::workflow::step_identity::{TOOL_SEED_USES, TOOLS_CACHE_RESTORE_USES};

/// Validate an `owner/repo@<40 hex>` action ref. Branch names are rejected.
///
/// Only registered tool-seed, tools-restore, and provider-admission composites
/// are accepted as local paths. Every other local path is rejected.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for a missing pin, a malformed
/// name, a forbidden installer action, or a short ref.
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    if matches!(
        uses,
        TOOL_SEED_USES | TOOLS_CACHE_RESTORE_USES | crate::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
    ) {
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
