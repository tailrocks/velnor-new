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

/// Provider-cache key prefix (per-root: target + tofu + root slug).
pub(crate) const TOFU_PROVIDERS_KEY_PREFIX: &str =
    velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDERS_KEY_PREFIX;
/// Owned plugin-cache base (expression form; mirrors the renderer's).
pub(crate) const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";
/// Restore-output gate and path hygiene for the post-restore admission step.
const TOFU_PROVIDER_CACHE_ADMISSION_SCRIPT: &str = r#"set -eu; runner_temp=$(cd -P "$RUNNER_TEMP" && pwd -P); case "$TOFU_PROVIDER_CACHE_DIR" in "$runner_temp"/velnor/tofu-cache/*) ;; *) exit 1 ;; esac; cache_parent="$runner_temp/velnor/tofu-cache"; if [ -L "$runner_temp/velnor" ] || [ -L "$cache_parent" ]; then exit 1; fi; mkdir -p "$cache_parent"; if [ "$(cd -P "$cache_parent" && pwd -P)" != "$cache_parent" ]; then exit 1; fi; relative="${TOFU_PROVIDER_CACHE_DIR#"$runner_temp"/}"; case "$relative" in velnor/tofu-cache/*) ;; *) exit 1 ;; esac; slug="${relative#velnor/tofu-cache/}"; case "$slug" in ''|*/*|*[!A-Za-z0-9_-]*) exit 1 ;; esac; cache_dir="$cache_parent/$slug"; if [ "$TOFU_PROVIDER_CACHE_DIR" != "$cache_dir" ]; then exit 1; fi; if [ "$TOFU_CACHE_HIT" = "true" ] && [ -n "$TOFU_EXPECTED_KEY" ] && [ "$TOFU_MATCHED_KEY" = "$TOFU_EXPECTED_KEY" ] && [ -d "$cache_dir" ] && [ ! -L "$cache_dir" ]; then exit 0; fi; rm -rf "$cache_dir"; mkdir -m 700 "$cache_dir""#;

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
    velnor_actions_tofu::tofu_root_slug(root)
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
    step.id = Some(StepId::TofuProviders);
    step.role = Some(StepRole::TofuProvidersRestore);
    Ok(step)
}

/// Admit only the exact cache entry returned by the same restore action.
///
/// The restore action may extract a prefix match before exposing its
/// outputs. On a miss or mismatched key, clear only the validated
/// job-private plugin-cache leaf so init can run cold. The generated
/// key binds target, OpenTofu pin, root slug, and lockfile hash; GitHub's
/// cache branch scope remains the trust boundary. `TF_DATA_DIR` is not
/// part of this path.
/// # Errors
///
/// Returns render errors for a rejected key or provider-cache path.
pub(crate) fn tofu_provider_cache_admission_step(
    key: &str,
    path: &str,
) -> Result<Step, OrchestratorError> {
    if key.trim().is_empty() || !velnor_actions_workflow_renderer::tofu_cache::tofu_providers_path_ok(path) {
        return Err(bad_key("bad_provider_admission_identity".to_owned()));
    }
    let env = std::collections::BTreeMap::from([
        (
            "TOFU_CACHE_HIT".to_owned(),
            format!("${{{{ steps.{}.outputs.cache-hit }}}}", StepId::TofuProviders.as_str()),
        ),
        ("TOFU_EXPECTED_KEY".to_owned(), key.to_owned()),
        (
            "TOFU_MATCHED_KEY".to_owned(),
            format!("${{{{ steps.{}.outputs.cache-matched-key }}}}", StepId::TofuProviders.as_str()),
        ),
        ("TOFU_PROVIDER_CACHE_DIR".to_owned(), path.to_owned()),
    ]);
    let script = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        TOFU_PROVIDER_CACHE_ADMISSION_SCRIPT.to_owned(),
    ];
    let mut step = velnor_actions_workflow_renderer::shell_step("Admit Tofu providers", script, env)
        .map_err(OrchestratorError::from)?;
    step.role = Some(StepRole::TofuProvidersAdmission);
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
    Ok(velnor_actions_tofu::root_for_key(&key))
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
) -> Result<Step, OrchestratorError> {
    let target = velnor_actions_contract::target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        }
    })?;
    let tofu = catalog.version(PinnedTool::Opentofu);
    let key = tofu_providers_cache_key(target, tofu, root)?;
    let path = tofu_provider_cache_path(root)?;
    tofu_providers_restore_step(&key, &path)
}

/// Restore followed by the same-key admission step for one root.
/// # Errors
///
/// Returns contract, actionlint, or render errors for invalid cache identity.
pub(crate) fn restore_and_admit_steps_for_tofu_root(
    label: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
    root: &str,
) -> Result<[Step; 2], OrchestratorError> {
    let target = velnor_actions_contract::target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("bad_label:{label}"),
        }
    })?;
    let tofu = catalog.version(PinnedTool::Opentofu);
    let key = tofu_providers_cache_key(target, tofu, root)?;
    let path = tofu_provider_cache_path(root)?;
    Ok([
        tofu_providers_restore_step(&key, &path)?,
        tofu_provider_cache_admission_step(&key, &path)?,
    ])
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
    fn provider_restore_is_exact_key_read_only() {
        let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "stacks/vpc")
            .expect("key builds");
        let path = tofu_provider_cache_path("stacks/vpc").expect("path builds");
        assert_eq!(
            TOFU_PROVIDER_CACHE_BASE_EXPR,
            velnor_actions_workflow_renderer::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR,
            "one base across crates"
        );
        assert!(
            path.starts_with(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/stacks-vpc-")),
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
    fn provider_admission_checks_same_restore_and_only_owns_plugin_cache() {
        let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "stacks/vpc")
            .expect("key builds");
        let path = tofu_provider_cache_path("stacks/vpc").expect("path builds");
        let step = tofu_provider_cache_admission_step(&key, &path).expect("admission builds");
        assert_eq!(step.name, "Admit Tofu providers");
        assert!(step.condition.is_none(), "every restore is admitted");
        let StepKind::Shell { run, env } = &step.kind else {
            panic!("admission must be a shell step");
        };
        assert!(
            env.iter().any(|(name, value)| {
                name == "TOFU_CACHE_HIT"
                    && value == "${{ steps.tofu-providers.outputs.cache-hit }}"
            }),
            "the actual restore hit output is observed"
        );
        assert!(
            env.iter().any(|(name, value)| {
                name == "TOFU_MATCHED_KEY"
                    && value == "${{ steps.tofu-providers.outputs.cache-matched-key }}"
            }),
            "the actual restore matched key is observed"
        );
        assert_eq!(
            env.get("TOFU_EXPECTED_KEY").map(String::as_str),
            Some(key.as_str()),
            "admission compares with the exact configured key"
        );
        assert_eq!(
            env.get("TOFU_PROVIDER_CACHE_DIR").map(String::as_str),
            Some(path.as_str()),
            "admission owns the restored plugin-cache leaf"
        );
        assert!(!env.contains_key("TF_DATA_DIR"), "data dir is untouched");
        let script = run.last().expect("admission script");
        for expected in [
            "[ \"TOFU_CACHE_HIT\" = \"true\" ]",
            "[ \"TOFU_MATCHED_KEY\" = \"$TOFU_EXPECTED_KEY\" ]",
            "rm -rf \"$cache_dir\"",
            "mkdir -m 700 \"$cache_dir\"",
        ] {
            assert!(script.contains(expected), "script lacks {expected:?}: {script}");
        }
    }

    #[test]
    fn provider_key_shape_binds_target_tofu_root_and_lock() {
        let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "")
            .expect("provider key builds");
        assert!(
            key.starts_with("velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-"),
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
