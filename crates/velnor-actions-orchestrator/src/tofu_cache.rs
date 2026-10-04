//! Exact source-owned provider keys and read-only consumer restores.
//!
//! The `tofu-providers` layer is acceleration only: a hit warms the
//! job-private plugin-cache dir before init, and never replaces
//! validate execution. Keys bind generation-captured source evidence and
//! the pure producer policy, scoped per validation root.

use velnor_actions_contract::{CrateObligation, Step};

use crate::OrchestratorError;
use crate::internal::internal;

/// Provider-cache key prefix (per-root: target + tofu + full root digest).
pub(crate) use velnor_actions_contract::cachekey::TOFU_PROVIDERS_KEY_PREFIX;
/// Owned plugin-cache base (expression form; mirrors the renderer's).
pub(crate) const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";

/// Per-root provider-cache key binds target, pin and pure source descriptor.
/// The descriptor digest includes immutable native lock bytes, exact public
/// selections and the compiled direct-only producer policy. Runtime repository
/// files cannot select transport entries, and no old consumer prefix is reused.
/// # Errors
///
/// Returns contract errors for unsupported targets, loose tofu pins,
/// unsafe or leading-dash roots, or overlong keys.
pub(crate) fn tofu_providers_cache_key(
    target: &str,
    tofu_version: &str,
    root: &str,
    source_digest: &str,
) -> Result<String, OrchestratorError> {
    use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(bad_key(format!("bad_target:{target}")));
    }
    velnor_actions_mise::validate_exact_version("opentofu", tofu_version).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    velnor_actions_tofu::validate_normalized_root(root)?;
    if root.starts_with('-') {
        return Err(bad_key(format!("leading_dash_root:{root}")));
    }
    if source_digest.len() != 64
        || !source_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(bad_key("invalid_provider_source_digest".to_owned()));
    }
    let component = provider_root_identity(root)?;
    let key =
        format!("{TOFU_PROVIDERS_KEY_PREFIX}-{target}-{tofu_version}-{component}-{source_digest}");
    if key.len() > MAX_CACHE_KEY_BYTES {
        return Err(bad_key("key_too_long".to_owned()));
    }
    Ok(key)
}

/// Domain-separated bounded locator; payload admission also proves the exact root.
fn provider_root_identity(root: &str) -> Result<String, OrchestratorError> {
    let key = velnor_actions_tofu::key_for_root(root);
    velnor_actions_tofu::root_for_key(&key)?;
    Ok(velnor_actions_contract::digest_b3(
        format!("tofu-provider-root-v1\n{key}").as_bytes(),
    ))
}

/// Job-private plugin-cache path for one root (never the data dir).
/// # Errors
///
/// Returns contract errors for an empty base (unreachable: the base
/// is a literal) or a root the adapter rejects.
pub(crate) fn tofu_provider_cache_path(root: &str) -> Result<String, OrchestratorError> {
    velnor_actions_tofu::tofu_cache_dir_under(TOFU_PROVIDER_CACHE_BASE_EXPR, root).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

/// Per-root provider restore step (L2 exact-key, no restore prefix).
/// # Errors
///
/// Returns contract, actionlint, or render errors for rejected pins,
/// keys, or step shapes.
pub(crate) fn tofu_providers_restore_step(
    key: &str,
    path: &str,
) -> Result<Step, OrchestratorError> {
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};
    let uses = PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let mut step = velnor_actions_workflow_renderer::steps::cache_action_step(
        true,
        &uses,
        "tofu-providers",
        key,
        &[],
        &[path.to_owned()],
    )
    .map_err(OrchestratorError::from)?;
    velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDERS_RESTORE_NAME
        .clone_into(&mut step.name);
    Ok(step)
}

/// Normalized tofu root backing one job's obligations.
///
/// Groups key by unit, so every member shares the root; the first
/// member's key segment names it.
/// # Errors
///
/// Returns an internal error for empty obligations or an unparsable
/// key segment (both unreachable past validation).
pub(crate) fn tofu_root_for_obligations(
    obligations: &[CrateObligation],
) -> Result<String, OrchestratorError> {
    let first = obligations
        .first()
        .ok_or_else(|| internal("tofu_empty_obligations"))?;
    let key = crate::extension_schemas::task_key_segment(&first.task_id)
        .ok_or_else(|| internal("tofu_unparsable_key"))?;
    velnor_actions_tofu::root_for_key(&key).map_err(OrchestratorError::from)
}

/// Restore step for one tofu root: key plus job-private path.
/// # Errors
///
/// Returns contract, actionlint, or render errors for bad labels,
/// pins, keys, paths, or step shapes.
pub(crate) fn restore_step_for_tofu_root(
    label: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
    root: &str,
    descriptor: &crate::tofu_cache_source::ProviderExportDescriptor,
) -> Result<Step, OrchestratorError> {
    let key = crate::tofu_producer_job::source_key(descriptor, catalog, label, root)?;
    let path = tofu_provider_cache_path(root)?;
    tofu_providers_restore_step(&key, &path)
}

/// Provider-cache key rejection.
fn bad_key(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}

/// Public roots restore providers; every selected root materializes isolation.
pub(crate) fn prepare_root_steps(
    label: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
    root: &str,
    descriptor: Option<&crate::tofu_cache_source::ProviderExportDescriptor>,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::new();
    if let Some(descriptor) = descriptor {
        steps.push(restore_step_for_tofu_root(
            label, catalog, root, descriptor,
        )?);
    }
    steps.push(crate::tofu_config_step::materialization_step(root)?);
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    #[test]
    fn provider_restore_is_exact_key_read_only() {
        let key = tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "stacks/vpc",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("key builds");
        let path = tofu_provider_cache_path("stacks/vpc").expect("path builds");
        assert_eq!(
            TOFU_PROVIDER_CACHE_BASE_EXPR,
            velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR,
            "one base across crates"
        );
        assert!(
            path.starts_with(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/b3-")),
            "{path}"
        );
        let step = tofu_providers_restore_step(&key, &path).expect("restore builds");
        assert_eq!(step.name, "Restore Tofu providers");
        assert!(step.condition.is_none(), "restores carry no gate");
        let StepKind::Action { uses, with, .. } = &step.kind else {
            panic!("restore must be an action step");
        };
        assert!(uses.starts_with("actions/cache/restore@"), "{uses}");
        assert_eq!(with.get("key").map(String::as_str), Some(key.as_str()));
        assert_eq!(
            with.get("restore-keys").map(String::as_str),
            Some(""),
            "L2 exact-key restore carries no prefix"
        );
    }

    #[test]
    fn provider_key_shape_binds_target_tofu_root_and_source() {
        let key = tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("provider key builds");
        assert!(
            key.starts_with("velnor-v2-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-b3-"),
            "{key}"
        );
        assert!(
            key.ends_with("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "{key}"
        );
        assert!(
            key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES,
            "{key}"
        );
    }
}
