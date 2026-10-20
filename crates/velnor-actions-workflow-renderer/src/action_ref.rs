//! Action `uses` checks. Fixed local renderer actions are allowed; every other
//! ref must be `owner/repo` at a 40-hex commit.

use crate::RenderError;
use velnor_actions_contract::workflow::step_identity::{
    CACHE_TAR_LAUNCHER_USES, TOOL_SEED_USES, TOOLS_CACHE_PRELUDE_USES, TOOLS_CACHE_RESTORE_USES,
};

/// Prefix for renderer-generated shared `ToFu` setup composites.
pub(crate) const TOFU_PROVIDER_PRELUDE_ACTION_PREFIX: &str =
    "./.github/actions/tofu-provider-prelude-";
/// Prefix for renderer-generated declared-task argv composites.
pub(crate) const DECLARED_TASK_ACTION_PREFIX: &str = "./.github/actions/declared-task-";

/// Validate an `owner/repo@<40 hex>` action ref. Branch names are rejected.
///
/// Only the fixed tool-seed, tools-cache, provider-admission, and cache-tar
/// composites use local paths. Every other local path is rejected.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for a missing pin, a malformed
/// name, a forbidden installer action, or a short ref.
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    if matches!(
        uses,
        CACHE_TAR_LAUNCHER_USES
            | TOOL_SEED_USES
            | TOOLS_CACHE_RESTORE_USES
            | crate::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
    ) || TOOLS_CACHE_PRELUDE_USES.contains(&uses)
        || is_generated_provider_prelude(uses)
        || is_qualification_cache_composite(uses)
        || is_crate_obligation_action(uses)
        || is_generated_declared_task(uses)
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

/// Prefix for renderer-generated crate obligation composites.
pub(crate) const CRATE_OBLIGATION_ACTION_PREFIX: &str = "./.github/actions/crate-obligation-";

/// Local composite for one validated qualification-cache owner and phase.
const QUALIFICATION_CACHE_COMPOSITE_PREFIX: &str = "./.github/actions/qualification-cache-";

pub(crate) fn is_qualification_cache_composite(uses: &str) -> bool {
    let Some(suffix) = uses.strip_prefix(QUALIFICATION_CACHE_COMPOSITE_PREFIX) else {
        return false;
    };
    if let Some(owner) = suffix.strip_prefix("prepare-") {
        return velnor_actions_contract::validate_job_id(owner).is_ok();
    }
    let Some((phase, rest)) = suffix.split_once('-') else {
        return false;
    };
    if !matches!(phase, "before" | "after") {
        return false;
    }
    let Some(rest) = rest.strip_prefix("save-") else {
        return false;
    };
    let Some((layer, owner)) = rest.split_once('-') else {
        return false;
    };
    matches!(
        layer,
        "mbx_objects" | "cargo_sources" | "mise_tools" | "tofu_providers" | "task_result"
    ) && velnor_actions_contract::validate_job_id(owner).is_ok()
}

/// Local composite that holds one crate job's obligation steps.
///
/// Obligation composites live in their own dedicated namespace so they never
/// collide with or reserve valid crate/root job IDs (such as `OpenTofu` roots
/// starting with `provider-prelude-`).
pub(crate) fn is_crate_obligation_action(uses: &str) -> bool {
    let Some(id) = uses.strip_prefix(CRATE_OBLIGATION_ACTION_PREFIX) else {
        return false;
    };
    !id.is_empty()
        && velnor_actions_contract::is_crate_job_id(id)
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
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

pub(crate) fn is_generated_declared_task(uses: &str) -> bool {
    uses.strip_prefix(DECLARED_TASK_ACTION_PREFIX)
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
    use velnor_actions_contract::workflow::step_identity::{
        CACHE_TAR_LAUNCHER_USES, TOOLS_CACHE_PRELUDE_USES,
    };

    #[test]
    fn only_registered_cache_composites_are_local_actions() {
        for uses in TOOLS_CACHE_PRELUDE_USES {
            assert!(validate_uses(uses).is_ok(), "{uses}");
        }
        assert!(validate_uses(CACHE_TAR_LAUNCHER_USES).is_ok());
        assert!(validate_uses("./.github/actions/tofu-provider-prelude-0").is_ok());
        assert!(validate_uses("./.github/actions/tofu-provider-prelude-12").is_ok());
        assert!(validate_uses("./.github/actions/crate-obligation-rust-demo").is_ok());
        assert!(validate_uses("./.github/actions/crate-obligation-tofu-stacks-r000").is_ok());
        assert!(
            validate_uses("./.github/actions/crate-obligation-tofu-provider-prelude-a").is_ok()
        );
        assert!(validate_uses("./.github/actions/declared-task-0").is_ok());
        assert!(validate_uses("./.github/actions/declared-task-12").is_ok());
        for uses in [
            "./.github/actions/rust-demo",
            "./.github/actions/velnor-tools-prelude-u20",
            "./.github/actions/velnor-tools-prelude-u26/other",
            "./.github/actions/velnor-tools-prelude-u26@0123456789abcdef",
            "./.github/actions/tofu-provider-prelude-",
            "./.github/actions/tofu-provider-prelude-01",
            "./.github/actions/tofu-provider-prelude-1-extra",
            "./.github/actions/crate-obligation-",
            "./.github/actions/declared-task-",
            "./.github/actions/declared-task-01",
            "./.github/actions/declared-task-1/extra",
        ] {
            assert!(validate_uses(uses).is_err(), "{uses}");
        }
    }
}
