//! Action `uses` checks. Fixed local renderer actions are allowed; every other
//! ref must be `owner/repo` at a 40-hex commit.

use crate::RenderError;
use velnor_actions_contract::workflow::step_identity::{
    TOOL_SEED_USES, TOOLS_CACHE_PRELUDE_USES, TOOLS_CACHE_RESTORE_USES,
};

/// Prefix for renderer-generated shared `ToFu` setup composites.
pub(crate) const TOFU_PROVIDER_PRELUDE_ACTION_PREFIX: &str =
    "./.github/actions/tofu-provider-prelude-";

/// Validate an `owner/repo@<40 hex>` action ref. Branch names are rejected.
///
/// Only registered tool-seed, tools-cache prelude/restore, and provider-
/// admission composites are accepted as local paths. Every other local path
/// is rejected.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for a missing pin, a malformed
/// name, a forbidden installer action, or a short ref.
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    if matches!(
        uses,
        TOOL_SEED_USES | TOOLS_CACHE_RESTORE_USES | crate::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
    ) || TOOLS_CACHE_PRELUDE_USES.contains(&uses)
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

pub(crate) fn is_generated_provider_prelude(uses: &str) -> bool {
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
mod tests {
    use super::validate_uses;
    use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_PRELUDE_USES;

    #[test]
    fn only_registered_cache_composites_are_local_actions() {
        for uses in TOOLS_CACHE_PRELUDE_USES {
            assert!(validate_uses(uses).is_ok(), "{uses}");
        }
        assert!(validate_uses("./.github/actions/tofu-provider-prelude-0").is_ok());
        assert!(validate_uses("./.github/actions/tofu-provider-prelude-12").is_ok());
        for uses in [
            "./.github/actions/velnor-tools-prelude-u20",
            "./.github/actions/velnor-tools-prelude-u26/other",
            "./.github/actions/velnor-tools-prelude-u26@0123456789abcdef",
            "./.github/actions/tofu-provider-prelude-",
            "./.github/actions/tofu-provider-prelude-01",
            "./.github/actions/tofu-provider-prelude-1-extra",
        ] {
            assert!(validate_uses(uses).is_err(), "{uses}");
        }
    }
}
