//! T21 provider-cache constructors: per-root key plus restore/save steps.
//!
//! The `tofu-providers` layer is acceleration only: a hit warms the
//! job-private plugin-cache dir before init, and never replaces
//! validate execution. Keys mirror the sources transport (static
//! segments plus a `hashFiles` snapshot), scoped per validation root.

use velnor_actions_contract::{CrateObligation, Step, StepId, StepRole};
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;
use crate::internal::internal;

/// Provider-cache key prefix (per-root: target + tofu + root locator).
pub(crate) const TOFU_PROVIDERS_KEY_PREFIX: &str =
    velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDERS_KEY_PREFIX;
/// Owned plugin-cache base (expression form; mirrors the renderer's).
pub(crate) const TOFU_PROVIDER_CACHE_BASE_EXPR: &str =
    velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR;
/// Per-root provider-cache key: target + tofu + opaque root locator + lock hash.
///
/// Static segments invalidate exactly when pins or the root change;
/// the trailing `hashFiles` over the root lockfile churns the key
/// when provider pins change. No spaces: the cache action rejects
/// them. The locator mirrors the isolated data-dir scheme and never
/// interpolates repository paths. Depth budget: roots carry no
/// separate depth cap; the 512-byte key cap bounds them instead — a
/// root nested deep enough to overflow the key fails `key_too_long`.
/// # Errors
///
/// Returns contract errors for unsupported targets, loose tofu pins,
/// unsafe or leading-dash roots, or overlong keys.
pub(crate) fn tofu_providers_cache_key(
    target: &str,
    tofu_version: &str,
    root: &str,
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
    crate::source_prep::validate_root(root)?;
    if root.starts_with('-') {
        return Err(bad_key(format!("leading_dash_root:{root}")));
    }
    let lock = if root.is_empty() {
        ".terraform.lock.hcl".to_owned()
    } else {
        format!("{root}/.terraform.lock.hcl")
    };
    let locator = velnor_actions_tofu::tofu_root_locator(root)?;
    let key = format!(
        "{TOFU_PROVIDERS_KEY_PREFIX}-{target}-{tofu_version}-{locator}-${{{{hashFiles('{lock}')}}}}"
    );
    if key.len() > MAX_CACHE_KEY_BYTES {
        return Err(bad_key("key_too_long".to_owned()));
    }
    Ok(key)
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
        || !velnor_actions_workflow_renderer::tofu_cache::tofu_providers_path_ok(path)
    {
        return Err(bad_key("bad_provider_restore_identity".to_owned()));
    }
    let mut step = velnor_actions_workflow_renderer::steps::action_step(
        velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDERS_RESTORE_NAME,
        velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_ADMISSION_USES,
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
/// key binds target, `OpenTofu` pin, root locator, and lockfile hash; GitHub's
/// cache branch scope remains the trust boundary. `TF_DATA_DIR` is not
/// part of this path.
/// # Errors
///
/// Returns render errors for a rejected key or provider-cache path.
/// Normalized tofu root backing one job's obligations.
///
/// Tofu obligations in one job must share one exact root; a present
/// Validate obligation gates on Init when that obligation is present.
/// # Errors
///
/// Rejects empty, duplicate, malformed, inconsistent, or mixed-root
/// Tofu obligations.
pub(crate) fn tofu_root_for_obligations(
    obligations: &[CrateObligation],
) -> Result<String, OrchestratorError> {
    use std::collections::BTreeSet;
    use velnor_actions_tofu::TofuTaskKind;

    let mut tofu = obligations.iter().filter(|obligation| {
        crate::extension_schemas::task_stack_segment(&obligation.task_id)
            == Some(velnor_actions_tofu::STACK_ID)
    });
    let first = tofu
        .next()
        .ok_or_else(|| internal("tofu_empty_obligations"))?;
    let (root, configuration, first_kind) = tofu_obligation_identity(first)?;
    let mut kinds = BTreeSet::from([first_kind.as_str()]);
    let mut identified = vec![(first, first_kind)];

    for obligation in tofu {
        let (other_root, other_configuration, kind) = tofu_obligation_identity(obligation)?;
        if other_root != root || other_configuration != configuration {
            return Err(internal("tofu_mixed_roots"));
        }
        if !kinds.insert(kind.as_str()) {
            return Err(internal("tofu_duplicate_obligation"));
        }
        identified.push((obligation, kind));
    }
    let has_init = kinds.contains(TofuTaskKind::InitForValidate.as_str());
    for (obligation, kind) in identified {
        validate_tofu_obligation_gate(obligation, kind, &root, &configuration, has_init)?;
    }
    Ok(root)
}

/// Parse and cross-check one Tofu obligation's exact root task identity.
fn tofu_obligation_identity(
    obligation: &CrateObligation,
) -> Result<(String, String, velnor_actions_tofu::TofuTaskKind), OrchestratorError> {
    use velnor_actions_tofu::TofuTaskKind;

    let parts: Vec<_> = obligation.task_id.split('/').collect();
    if parts.len() != 5 || parts[0] != "stack" || parts[1] != velnor_actions_tofu::STACK_ID {
        return Err(internal("tofu_unparsable_task_id"));
    }
    let root = velnor_actions_tofu::root_for_key(parts[2])?;
    let kind = TofuTaskKind::parse(parts[3])?;
    if obligation.kind != kind.as_str()
        || velnor_actions_tofu::task_id_for_root(&root, kind, parts[4])? != obligation.task_id
    {
        return Err(internal("tofu_obligation_identity_mismatch"));
    }
    Ok((root, parts[4].to_owned(), kind))
}

/// Check the fixed Tofu dependency contract carried by one obligation.
fn validate_tofu_obligation_gate(
    obligation: &CrateObligation,
    kind: velnor_actions_tofu::TofuTaskKind,
    root: &str,
    configuration: &str,
    has_init: bool,
) -> Result<(), OrchestratorError> {
    use velnor_actions_tofu::TofuTaskKind;

    let expected = match kind {
        TofuTaskKind::Validate if has_init => vec![velnor_actions_tofu::task_id_for_root(
            root,
            TofuTaskKind::InitForValidate,
            configuration,
        )?],
        TofuTaskKind::Validate | TofuTaskKind::Fmt | TofuTaskKind::InitForValidate => Vec::new(),
    };
    if obligation.gated_by != expected {
        return Err(internal("tofu_obligation_gate_mismatch"));
    }
    Ok(())
}

/// Build the compact restore/admission step for one root.
/// # Errors
///
/// Returns contract, actionlint, or render errors for invalid cache identity.
pub(crate) fn provider_cache_step_for_tofu_root(
    label: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
    root: &str,
) -> Result<[Step; 1], OrchestratorError> {
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
        .map(velnor_actions_contract::ReleaseTarget::triple)
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
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    #[test]
    fn provider_restore_composite_binds_exact_key_and_owned_path() {
        let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "stacks/vpc")
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
        assert_eq!(
            uses,
            velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
        );
        assert_eq!(
            with.get("cache-key").map(String::as_str),
            Some(key.as_str()),
            "the composite receives the configured key once"
        );
        assert_eq!(
            with.get("cache-path").map(String::as_str),
            Some(path.as_str()),
            "the composite receives only the owned plugin-cache path"
        );
        let script = velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_ADMISSION_SCRIPT;
        for expected in [
            "[ \"$TOFU_CACHE_HIT\" = true ]",
            "[ \"$TOFU_MATCHED_KEY\" = \"$TOFU_EXPECTED_KEY\" ]",
            "\"$TOFU_PROVIDER_CACHE_PATH\" = \"$d\"",
            "rm -rf \"$d\"",
            "mkdir -m 700 \"$d\"",
        ] {
            assert!(
                script.contains(expected),
                "script lacks {expected:?}: {script}"
            );
        }
        assert!(!script.contains("TF_DATA_DIR"), "data dir is untouched");
    }

    #[test]
    fn provider_key_shape_binds_target_tofu_root_and_lock() {
        let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "")
            .expect("provider key builds");
        assert!(
            key.starts_with("velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-b3-"),
            "{key}"
        );
        assert!(
            key.ends_with("${{hashFiles('.terraform.lock.hcl')}}"),
            "{key}"
        );
        assert!(
            key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES,
            "{key}"
        );
    }
}
