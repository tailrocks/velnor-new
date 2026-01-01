//! Action `uses` checks. Fixed internal local composites are allowed; every
//! remote ref must be `owner/repo` at a 40-hex commit.

use crate::RenderError;
use velnor_actions_contract_workflow::workflow::step_identity::{
    TOFU_PROVIDER_ADMISSION_USES, TOOL_SEED_USES,
};

/// Prefix for renderer-generated shared `ToFu` setup composites.
pub const TOFU_PROVIDER_PRELUDE_ACTION_PREFIX: &str = "./.github/actions/tofu-provider-prelude-";
/// Full-SHA Alint pin for the repository-policy `alint` job.
pub const ALINT_USES: &str = "asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb";
/// Pinned Alint binary release tag for the step's `version:` input.
///
/// Per the action's `action.yml`, a SHA-pinned `uses:` falls back to
/// installing `latest` unless `version:` is set — a floating binary. Mirror of
/// `ALINT_ACTION_VERSION` (`velnor-actions-actionlint`, same qualified
/// release); the renderer cannot depend on that crate, so
/// `scripts/check-freshness.sh` pins this mirror to the reviewed
/// `asamarts/alint` inventory row instead of trusting the duplication.
pub const ALINT_BINARY_VERSION: &str = "v0.16.1";

/// Validate an `owner/repo@<40 hex>` action ref. Branch names are rejected.
///
/// The tool seed, provider admission, and generated provider-prelude composites
/// are the only local paths this renderer emits. Every other local path is rejected.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for a missing pin, a malformed
/// name, a forbidden installer action, or a short ref.
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    if matches!(uses, TOOL_SEED_USES | TOFU_PROVIDER_ADMISSION_USES)
        || is_generated_provider_prelude(uses)
    {
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

/// True for renderer-generated shared `ToFu` setup composite refs.
#[must_use]
pub fn is_generated_provider_prelude(uses: &str) -> bool {
    uses.strip_prefix(TOFU_PROVIDER_PRELUDE_ACTION_PREFIX)
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix.bytes().all(|byte| byte.is_ascii_digit())
                && suffix
                    .parse::<usize>()
                    .is_ok_and(|index| index.to_string() == suffix)
        })
}

/// True for `owner/repo` over alphanumerics plus `.-_`.
fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}
#[cfg(test)]
mod tests;
