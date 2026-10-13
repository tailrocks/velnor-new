//! Cross-step provider-cache ownership and ordering contracts.

use std::collections::BTreeMap;

use super::{
    ir::CACHE_SAVE_CONDITION,
    step::{Step, StepKind},
    step_identity::{
        StepId, StepRole, TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDER_CACHE_BASE_EXPR,
        TOFU_PROVIDERS_KEY_OUTPUT_EXPR, TOFU_PROVIDERS_KEY_PREFIX, TOFU_PROVIDERS_PATH_OUTPUT_EXPR,
    },
};
use crate::errors::ContractError;

#[derive(Default)]
struct ProtocolSteps {
    restores: Vec<usize>,
    consumers: Vec<usize>,
    saves: Vec<usize>,
}

struct ProviderBinding<'a> {
    key: &'a str,
    path: &'a str,
    restore_index: usize,
}

/// Validate the exact provider restore/admission, use, and save sequence.
pub(super) fn validate_tofu_provider_sequence(
    steps: &[Step],
    scope: &str,
) -> Result<(), ContractError> {
    let roles = classify_protocol_steps(steps, scope)?;
    if !roles.present() {
        return Ok(());
    }
    let binding = validate_restore_admission(steps, &roles, scope)?;
    validate_provider_consumers(steps, &roles.consumers, &binding, scope)?;
    validate_provider_save(steps, &roles, &binding, scope)
}

/// Find protocol steps and reject untyped provider-cache payloads.
fn classify_protocol_steps(steps: &[Step], scope: &str) -> Result<ProtocolSteps, ContractError> {
    let mut roles = ProtocolSteps::default();
    for (index, step) in steps.iter().enumerate() {
        match step.role {
            Some(StepRole::TofuProvidersRestore) => roles.restores.push(index),
            Some(StepRole::TofuProviderUse) => roles.consumers.push(index),
            Some(StepRole::TofuProvidersSave) => roles.saves.push(index),
            _ => {}
        }
        let uses_provider_env = has_provider_env(step);
        if uses_provider_env && step.role != Some(StepRole::TofuProviderUse) {
            return Err(invalid(scope, "tofu_provider_use_role_missing"));
        }
        if is_provider_cache_action(step)
            && !matches!(
                step.role,
                Some(StepRole::TofuProvidersRestore | StepRole::TofuProvidersSave)
            )
        {
            return Err(invalid(scope, "tofu_provider_cache_role_missing"));
        }
    }
    Ok(roles)
}

impl ProtocolSteps {
    /// True when any typed provider-cache step participates in the job.
    fn present(&self) -> bool {
        !self.restores.is_empty() || !self.consumers.is_empty() || !self.saves.is_empty()
    }
}

/// Bind one unconditional restore/admission composite to its output owner.
fn validate_restore_admission<'a>(
    steps: &'a [Step],
    roles: &ProtocolSteps,
    scope: &str,
) -> Result<ProviderBinding<'a>, ContractError> {
    if roles.restores.len() != 1 {
        return Err(invalid(scope, "tofu_provider_restore_count"));
    }
    let restore_index = roles.restores[0];
    let restore = &steps[restore_index];
    if restore.condition.is_some() {
        return Err(invalid(scope, "tofu_provider_restore_conditional"));
    }
    let restore_with = action_with(restore).ok_or_else(|| invalid(scope, "tofu_restore_shape"))?;
    let key = restore_with
        .get("cache-key")
        .map(String::as_str)
        .ok_or_else(|| invalid(scope, "tofu_restore_key_missing"))?;
    let path = restore_with
        .get("cache-path")
        .map(String::as_str)
        .ok_or_else(|| invalid(scope, "tofu_restore_path_missing"))?;
    let slug =
        provider_path_slug(path).ok_or_else(|| invalid(scope, "tofu_provider_path_invalid"))?;
    if !valid_provider_cache_key(key, slug) {
        return Err(invalid(scope, "tofu_provider_key_invalid"));
    }
    Ok(ProviderBinding {
        key,
        path,
        restore_index,
    })
}

/// Ensure every provider-cache consumer is ordered after admission and owns the path.
fn validate_provider_consumers(
    steps: &[Step],
    consumers: &[usize],
    binding: &ProviderBinding<'_>,
    scope: &str,
) -> Result<(), ContractError> {
    for index in consumers.iter().copied() {
        let consumer = &steps[index];
        let env = step_env(consumer).ok_or_else(|| invalid(scope, "tofu_provider_use_env"))?;
        if index <= binding.restore_index {
            return Err(invalid(scope, "tofu_provider_use_before_admission"));
        }
        if env.get("TF_PLUGIN_CACHE_DIR").map(String::as_str) != Some(binding.path) {
            return Err(invalid(scope, "tofu_provider_use_path_mismatch"));
        }
        if env.get("TF_DATA_DIR").is_none_or(|data_dir| {
            data_dir == binding.path || data_dir.starts_with(&format!("{}/", binding.path))
        }) {
            return Err(invalid(scope, "tofu_data_dir_provider_cache_alias"));
        }
    }
    Ok(())
}

/// Verify the single optional elected save shares the restore's key and path.
fn validate_provider_save(
    steps: &[Step],
    roles: &ProtocolSteps,
    binding: &ProviderBinding<'_>,
    scope: &str,
) -> Result<(), ContractError> {
    if roles.saves.len() > 1 {
        return Err(invalid(scope, "tofu_provider_save_count"));
    }
    if let Some(index) = roles.saves.first().copied() {
        if index <= binding.restore_index {
            return Err(invalid(scope, "tofu_provider_save_before_restore"));
        }
        let save = &steps[index];
        let save_with = action_with(save).ok_or_else(|| invalid(scope, "tofu_save_shape"))?;
        let restore = &steps[binding.restore_index];
        let restore_with =
            action_with(restore).ok_or_else(|| invalid(scope, "tofu_restore_shape"))?;
        if restore_with.get("cache-key").map(String::as_str) != Some(binding.key)
            || restore_with.get("cache-path").map(String::as_str) != Some(binding.path)
        {
            return Err(invalid(scope, "tofu_provider_save_binding_mismatch"));
        }
        let expected_key = output_reference(restore, "cache-key")
            .ok_or_else(|| invalid(scope, "tofu_provider_restore_id_missing"))?;
        let expected_path = output_reference(restore, "cache-path")
            .ok_or_else(|| invalid(scope, "tofu_provider_restore_id_missing"))?;
        if save_with.get("key") != Some(&expected_key)
            || save_with.get("path") != Some(&expected_path)
        {
            return Err(invalid(scope, "tofu_provider_save_binding_mismatch"));
        }
        if save.condition.as_deref() != Some(CACHE_SAVE_CONDITION) {
            return Err(invalid(scope, "tofu_provider_save_gate_mismatch"));
        }
        if roles.consumers.iter().any(|consumer| index <= *consumer) {
            return Err(invalid(scope, "tofu_provider_save_before_use"));
        }
    }
    Ok(())
}

/// Reference one output of the exact typed composite that admitted the cache.
fn output_reference(step: &Step, output: &str) -> Option<String> {
    let id = step.id?;
    Some(format!("${{{{ steps.{}.outputs.{output} }}}}", id.as_str()))
}

/// Validate one provider restore payload before cross-step binding.
pub(crate) fn valid_provider_restore(kind: &StepKind) -> bool {
    let StepKind::Action { uses, with, env } = kind else {
        return false;
    };
    let Some(key) = with.get("cache-key") else {
        return false;
    };
    let Some(path) = with.get("cache-path") else {
        return false;
    };
    with.len() == 2
        && uses == TOFU_PROVIDER_ADMISSION_USES
        && env.is_empty()
        && provider_path_slug(path).is_some_and(|slug| valid_provider_cache_key(key, slug))
}

/// Validate one provider save payload before cross-step binding.
pub(crate) fn valid_provider_save(kind: &StepKind) -> bool {
    let StepKind::Action { uses, with, env } = kind else {
        return false;
    };
    let Some(key) = with.get("key") else {
        return false;
    };
    let Some(path) = with.get("path") else {
        return false;
    };
    with.len() == 2
        && uses.starts_with("actions/cache/save@")
        && env.is_empty()
        && key == TOFU_PROVIDERS_KEY_OUTPUT_EXPR
        && path == TOFU_PROVIDERS_PATH_OUTPUT_EXPR
}

/// Validate a shell step that consumes the admitted provider cache.
pub(crate) fn valid_provider_use(kind: &StepKind) -> bool {
    let StepKind::Shell { env, .. } = kind else {
        return false;
    };
    let Some(path) = env.get("TF_PLUGIN_CACHE_DIR") else {
        return false;
    };
    provider_path_slug(path).is_some()
        && env.get("TF_DATA_DIR").is_some_and(|data_dir| {
            !data_dir.is_empty() && data_dir != path && !data_dir.starts_with(&format!("{path}/"))
        })
}

/// Validate one canonical provider-cache path and return its private leaf.
fn provider_path_slug(path: &str) -> Option<&str> {
    let slug = path.strip_prefix(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/"))?;
    (!slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(slug)
}

/// Require the full key to bind a supported target, exact `ToFu` pin, leaf, and lock file.
fn valid_provider_cache_key(key: &str, slug: &str) -> bool {
    if key.len() > crate::cachekey::MAX_CACHE_KEY_BYTES
        || key.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return false;
    }
    let Some((identity, lock_suffix)) = key
        .strip_prefix(&format!("{TOFU_PROVIDERS_KEY_PREFIX}-"))
        .and_then(|rest| rest.split_once("-${{hashFiles('"))
    else {
        return false;
    };
    let Some(lock_path) = lock_suffix.strip_suffix("')}}") else {
        return false;
    };
    if !valid_lock_path(lock_path) {
        return false;
    }
    let Some(identity) = identity.strip_suffix(&format!("-{slug}")) else {
        return false;
    };
    crate::targets::SUPPORTED_TARGETS.iter().any(|target| {
        identity
            .strip_prefix(&format!("{target}-"))
            .is_some_and(valid_exact_version)
    })
}

/// Validate the configured lock-file path used by `hashFiles`.
fn valid_lock_path(path: &str) -> bool {
    path.ends_with(".terraform.lock.hcl")
        && !path.starts_with('/')
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

/// Exact `major.minor.patch` version spelling used by the provider key.
fn valid_exact_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let is_numeric = |part: Option<&str>| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    is_numeric(parts.next())
        && is_numeric(parts.next())
        && is_numeric(parts.next())
        && parts.next().is_none()
}

/// True for an action or shell step with provider environment bindings.
fn has_provider_env(step: &Step) -> bool {
    step_env(step).is_some_and(|env| {
        env.contains_key("TF_PLUGIN_CACHE_DIR") || env.contains_key("TF_DATA_DIR")
    })
}

/// True for an untyped cache action that addresses the provider layer.
fn is_provider_cache_action(step: &Step) -> bool {
    if step.id == Some(StepId::TofuProviders) {
        return true;
    }
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    if uses == TOFU_PROVIDER_ADMISSION_USES {
        return true;
    }
    let cache_action =
        uses.starts_with("actions/cache/restore@") || uses.starts_with("actions/cache/save@");
    cache_action
        && (with
            .get("key")
            .is_some_and(|key| key.starts_with(TOFU_PROVIDERS_KEY_PREFIX))
            || with
                .get("path")
                .is_some_and(|path| path.contains(TOFU_PROVIDER_CACHE_BASE_EXPR)))
}

/// Return the input map for a typed cache action step.
fn action_with(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Action { with, .. } => Some(with),
        StepKind::Shell { .. } | StepKind::TaskExecution { .. } | StepKind::Internal { .. } => None,
    }
}

/// Return the explicit environment map for a shell or action step.
fn step_env(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Action { env, .. }
        | StepKind::Shell { env, .. }
        | StepKind::TaskExecution { env, .. } => Some(env),
        StepKind::Internal { .. } => None,
    }
}

/// Build a stable contract diagnostic scoped to one serialized job.
fn invalid(scope: &str, problem: &str) -> ContractError {
    ContractError::identity(
        "job.steps.tofu_provider_cache",
        format!("{problem}:{scope}"),
    )
}
