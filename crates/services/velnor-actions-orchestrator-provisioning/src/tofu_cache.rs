//! T21 provider-cache constructors: per-root key plus restore/save steps.
//!
//! The `tofu-providers` layer is acceleration only: a hit warms the
//! job-private plugin-cache dir before init, and never replaces
//! validate execution. Keys mirror the sources transport (static
//! segments plus a `hashFiles` snapshot), scoped per validation root.

use velnor_actions_contract_workflow::{CrateObligation, Step, StepId, StepRole};
use velnor_actions_mise::PinnedTool;

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;

/// Provider-cache key prefix (per-root: target + tofu + root slug).
pub(crate) const TOFU_PROVIDERS_KEY_PREFIX: &str =
    velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDERS_KEY_PREFIX;
/// Owned plugin-cache base (expression form; mirrors the renderer's).
pub(crate) const TOFU_PROVIDER_CACHE_BASE_EXPR: &str =
    velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR;
/// Per-root provider-cache key: target + tofu + root slug + lock hash.
///
/// Static segments invalidate exactly when pins or the root change;
/// the trailing `hashFiles` over the root lockfile churns the key
/// when provider pins change. No spaces: the cache action rejects
/// them. The root slug mirrors the isolated data-dir scheme
/// (H3-hashed, never interpolated). Depth budget: roots carry no
/// separate depth cap; the 512-byte key cap bounds them instead — a
/// root nested deep enough to overflow the key fails `key_too_long`.
/// # Errors
///
/// Returns contract errors for unsupported targets, loose tofu pins,
/// unsafe or leading-dash roots, or overlong keys.
pub fn tofu_providers_cache_key(
    target: &str,
    tofu_version: &str,
    root: &str,
) -> Result<String, OrchestratorError> {
    use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;
    if !velnor_actions_contract_release::is_supported_target(target) {
        return Err(bad_key(format!("bad_target:{target}")));
    }
    velnor_actions_mise::validate_exact_version("opentofu", tofu_version).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    crate::source_prep::validate_root(root)?;
    if root.starts_with('-') {
        return Err(bad_key(format!("leading_dash_root:{root}")));
    }
    let lock = if root.is_empty() {
        ".terraform.lock.hcl".to_owned()
    } else {
        format!("{root}/.terraform.lock.hcl")
    };
    let slug = provider_root_slug(root);
    let key = format!(
        "{TOFU_PROVIDERS_KEY_PREFIX}-{target}-{tofu_version}-{slug}-${{{{hashFiles('{lock}')}}}}"
    );
    if key.len() > MAX_CACHE_KEY_BYTES {
        return Err(bad_key("key_too_long".to_owned()));
    }
    Ok(key)
}

/// H3-hashed root slug through the tofu adapter's shared derivation.
fn provider_root_slug(root: &str) -> String {
    velnor_actions_tofu_core::tofu_root_slug(root)
}

/// Job-private plugin-cache path for one root (never the data dir).
/// # Errors
///
/// Returns contract errors for an empty base (unreachable: the base
/// is a literal) or a root the adapter rejects.
pub(crate) fn tofu_provider_cache_path(root: &str) -> Result<String, OrchestratorError> {
    velnor_actions_tofu_core::tofu_cache_dir_under(TOFU_PROVIDER_CACHE_BASE_EXPR, root).map_err(
        |err| OrchestratorError::Contract {
            problem: err.to_string(),
        },
    )
}

/// Per-root provider restore/admission composite (L2 exact-key).
/// # Errors
///
/// Returns contract, actionlint, or render errors for rejected pins,
/// keys, or step shapes.
pub(crate) fn tofu_providers_restore_step(
    key: &str,
    path: &str,
) -> Result<Step, OrchestratorError> {
    if key.trim().is_empty()
        || !velnor_actions_workflow_cache::tofu_cache::tofu_providers_path_ok(path)
    {
        return Err(bad_key("bad_provider_restore_identity".to_owned()));
    }
    let mut step = velnor_actions_workflow_steps::steps::action_step(
        velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDERS_RESTORE_NAME,
        velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_ADMISSION_USES,
        std::collections::BTreeMap::from([
            ("cache-key".to_owned(), key.to_owned()),
            ("cache-path".to_owned(), path.to_owned()),
        ]),
    )
    .map_err(OrchestratorError::from)?;
    step.id = Some(StepId::TofuProviders);
    step.role = Some(StepRole::TofuProvidersRestore);
    Ok(step)
}

/// Admit only the exact cache entry returned by the same restore action.
///
/// The restore action may extract a prefix match before exposing its
/// outputs. On a miss or mismatched key, clear only the validated
/// job-private plugin-cache leaf so init can run cold. The generated
/// key binds target, `OpenTofu` pin, root slug, and lockfile hash; GitHub's
/// cache branch scope remains the trust boundary. `TF_DATA_DIR` is not
/// part of this path.
/// # Errors
///
/// Returns render errors for a rejected key or provider-cache path.
/// Normalized tofu root backing one job's obligations.
///
/// Groups key by unit, so every member shares the root; the first
/// member's key segment names it.
/// # Errors
///
/// Returns an internal error for empty obligations or an unparsable
/// key segment (both unreachable past validation).
pub fn tofu_root_for_obligations(
    obligations: &[CrateObligation],
) -> Result<String, OrchestratorError> {
    let first = obligations
        .first()
        .ok_or_else(|| internal("tofu_empty_obligations"))?;
    let key = velnor_actions_orchestrator_core::extension_schemas::task_key_segment(&first.task_id)
        .ok_or_else(|| internal("tofu_unparsable_key"))?;
    Ok(velnor_actions_tofu_core::root_for_key(&key))
}

/// Build the compact restore/admission step for one root.
/// # Errors
///
/// Returns contract, actionlint, or render errors for invalid cache identity.
pub fn provider_cache_step_for_tofu_root(
    label: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
    root: &str,
) -> Result<[Step; 1], OrchestratorError> {
    let target = velnor_actions_contract_release::ReleaseTarget::for_runner_label(label)
        .map(velnor_actions_contract_release::ReleaseTarget::triple)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        })?;
    let tofu = catalog.version(PinnedTool::Opentofu);
    let key = tofu_providers_cache_key(target, tofu, root)?;
    let path = tofu_provider_cache_path(root)?;
    Ok([tofu_providers_restore_step(&key, &path)?])
}

/// Provider-cache key rejection.
fn bad_key(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}
#[cfg(test)]
mod tests;
